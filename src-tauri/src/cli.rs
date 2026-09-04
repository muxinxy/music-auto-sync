use std::{fs, path::PathBuf, sync::atomic::AtomicBool, sync::Arc};

use crate::{
    api::NeteaseApi,
    core::sync::{self, SingleDownloadOptions},
    store::{self, config::Config},
    AppState,
};

/// 若命令行包含 `--cli`，则以无窗口模式解析子命令并退出，返回 true 表示已处理。
pub fn run_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(index) = args.iter().position(|arg| arg == "--cli") else {
        return false;
    };
    let code = tokio::runtime::Runtime::new()
        .map(|runtime| runtime.block_on(run(&args[index + 1..])))
        .unwrap_or_else(|error| {
            eprintln!("failed to start runtime: {error}");
            1
        });
    std::process::exit(code);
}

fn build_state() -> Result<AppState, String> {
    let paths = store::paths::DataPaths::discover().map_err(|error| error.to_string())?;
    Ok(AppState {
        paths: store::AppPaths::new(paths),
        sync_running: AtomicBool::new(false),
        cancel_requested: Arc::new(AtomicBool::new(false)),
    })
}

fn print_json(value: &impl serde::Serialize, output: Option<&PathBuf>) {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into());
    if let Some(path) = output {
        let _ = fs::write(path, &text);
    }
    println!("{text}");
}

async fn run(args: &[String]) -> i32 {
    let mut output: Option<PathBuf> = None;
    let mut command_args: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--output" || arg == "-o" {
            if let Some(path) = args.get(index + 1) {
                output = Some(PathBuf::from(path));
                index += 2;
                continue;
            }
        }
        command_args.push(arg.clone());
        index += 1;
    }

    let Some(subcommand) = command_args.first() else {
        print_usage();
        return 2;
    };

    let state = match build_state() {
        Ok(state) => state,
        Err(error) => {
            print_json(&serde_json::json!({ "error": error }), output.as_ref());
            return 1;
        }
    };

    match subcommand.as_str() {
        "status" => run_status(&state, output.as_ref()).await,
        "sync" => run_sync(&state, &command_args[1..], output.as_ref()).await,
        "download" => run_download(&state, &command_args[1..], output.as_ref()).await,
        "help" | "-h" | "--help" => {
            print_usage();
            0
        }
        _ => {
            print_usage();
            2
        }
    }
}

async fn run_status(state: &AppState, output: Option<&PathBuf>) -> i32 {
    let config = match store::config::load(&state.paths.get().config_file) {
        Ok(config) => config,
        Err(error) => {
            print_json(&serde_json::json!({ "error": error.to_string() }), output);
            return 1;
        }
    };
    let api = match NeteaseApi::from_config(&config) {
        Ok(api) => api,
        Err(error) => {
            print_json(&serde_json::json!({ "error": error.to_string() }), output);
            return 1;
        }
    };
    match api.login_status().await {
        Ok(response) => {
            let status = &response.status;
            print_json(
                &serde_json::json!({
                    "loggedIn": status.logged_in,
                    "nickname": status.nickname,
                    "userId": status.user_id,
                }),
                output,
            );
            0
        }
        Err(error) => {
            print_json(&serde_json::json!({ "error": error.to_string() }), output);
            1
        }
    }
}

async fn run_sync(state: &AppState, args: &[String], output: Option<&PathBuf>) -> i32 {
    let config = match store::config::load(&state.paths.get().config_file) {
        Ok(config) => config,
        Err(error) => {
            print_json(&serde_json::json!({ "error": error.to_string() }), output);
            return 1;
        }
    };
    let ids: Vec<u64> = if let Some(first) = args.first() {
        if first == "all" {
            config
                .playlists
                .iter()
                .filter(|playlist| playlist.enabled)
                .map(|playlist| playlist.id)
                .collect()
        } else {
            match first.parse::<u64>() {
                Ok(id) => vec![id],
                Err(_) => {
                    print_json(
                        &serde_json::json!({ "error": "invalid playlist id" }),
                        output,
                    );
                    return 2;
                }
            }
        }
    } else {
        print_usage();
        return 2;
    };

    let mut reports: Vec<serde_json::Value> = Vec::new();
    let mut failed = 0usize;
    for id in ids {
        match sync::cli_sync(state, id).await {
            Ok(report) => reports
                .push(serde_json::to_value(&report).unwrap_or_else(|_| serde_json::json!({}))),
            Err(message) => {
                failed += 1;
                reports.push(serde_json::json!({
                    "playlistId": id,
                    "error": message,
                }));
            }
        }
    }
    print_json(
        &serde_json::json!({ "failed": failed, "reports": reports }),
        output,
    );
    if failed == 0 {
        0
    } else {
        1
    }
}

async fn run_download(state: &AppState, args: &[String], output: Option<&PathBuf>) -> i32 {
    let mut playlist_id: Option<u64> = None;
    let mut track_id: Option<u64> = None;
    let mut options = SingleDownloadOptions {
        target_dir: None,
        filename_template: None,
        quality: None,
        write_lrc: None,
        overwrite: false,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dir" => {
                if let Some(value) = args.get(index + 1) {
                    options.target_dir = Some(value.clone());
                    index += 2;
                    continue;
                }
            }
            "--quality" => {
                if let Some(value) = args.get(index + 1) {
                    options.quality = Some(value.clone());
                    index += 2;
                    continue;
                }
            }
            "--overwrite" => {
                options.overwrite = true;
                index += 1;
                continue;
            }
            "--no-lyrics" => {
                options.write_lrc = Some(false);
                index += 1;
                continue;
            }
            _ => {}
        }
        if playlist_id.is_none() {
            match args[index].parse::<u64>() {
                Ok(value) => playlist_id = Some(value),
                Err(_) => {
                    print_json(
                        &serde_json::json!({ "error": "invalid playlist id" }),
                        output,
                    );
                    return 2;
                }
            }
        } else if track_id.is_none() {
            match args[index].parse::<u64>() {
                Ok(value) => track_id = Some(value),
                Err(_) => {
                    print_json(&serde_json::json!({ "error": "invalid track id" }), output);
                    return 2;
                }
            }
        } else {
            print_usage();
            return 2;
        }
        index += 1;
    }

    let (Some(playlist_id), Some(track_id)) = (playlist_id, track_id) else {
        print_usage();
        return 2;
    };

    let config: Config = match store::config::load(&state.paths.get().config_file) {
        Ok(config) => config,
        Err(error) => {
            print_json(&serde_json::json!({ "error": error.to_string() }), output);
            return 1;
        }
    };
    if options.write_lrc.is_none() {
        options.write_lrc = Some(config.write_lrc);
    }

    match sync::download_song_with_options(None, state, playlist_id, track_id, options).await {
        Ok(path) => {
            print_json(&serde_json::json!({ "path": path }), output);
            0
        }
        Err(message) => {
            print_json(&serde_json::json!({ "error": message }), output);
            1
        }
    }
}

fn print_usage() {
    println!(
        "Music Auto Sync CLI

Usage:
  music-auto-sync --cli status
  music-auto-sync --cli sync <playlistId|all>
  music-auto-sync --cli download <playlistId> <trackId> [--dir <path>] [--quality <level>] [--overwrite] [--no-lyrics]
  music-auto-sync --cli --output <file> ...

Options:
  --output, -o <file>   Write the JSON result to a file in addition to stdout.
  --dir <path>          Save directory for the download.
  --quality <level>     standard | higher | exhigh | lossless | hires.
  --overwrite           Overwrite an existing file.
  --no-lyrics           Do not download lyrics.
"
    );
}
