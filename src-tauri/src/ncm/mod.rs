pub mod ncm;

use std::path::Path;

use crate::api::NeteaseApi;

/// 转换成功后按全局设置嵌入歌词（NCM 元数据不含歌词，需联网取词）。
/// 失败仅告警，不影响转换结果；仅对本次真正转换（而非收编跳过）的产物生效。
pub(crate) async fn embed_lyrics_after_convert(
    api: &NeteaseApi,
    enabled: bool,
    item: &ncm::NcmConvertItemResult,
) {
    if !enabled {
        return;
    }
    let (Some(id), Some(output)) = (item.music_id, item.output.as_deref()) else {
        return;
    };
    if item.status != "converted" || !Path::new(output).is_file() {
        return;
    }
    match api.lyric(id).await {
        Ok(Some(lyrics)) => {
            if let Err(error) = crate::tags::tags::write_embedded_lyrics(Path::new(output), &lyrics) {
                tracing::warn!(%error, path = output, "ncm lyrics embed failed");
            }
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, id, "ncm lyrics fetch failed"),
    }
}
