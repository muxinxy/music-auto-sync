import { useEffect, useRef, useState } from "react";
import { CloseOutlined, SearchOutlined } from "@ant-design/icons";
import { Dropdown, Input, Space, Typography, theme } from "antd";
import type { InputRef } from "antd";
import i18n from "./i18n";
import {
  clearSearchHistory,
  loadSearchHistory,
  pushSearchHistory,
  removeSearchHistory,
} from "./filterMemory";

/**
 * 带历史记录的搜索框：聚焦时下拉最近 8 个关键词，可点选复用、
 * 单条右侧 X 删除、底部一键清空全部历史。
 */
export default function SearchWithHistory({
  storageKey,
  value,
  onChange,
  onSearch,
  placeholder,
  style,
}: {
  storageKey: string;
  value: string;
  onChange: (value: string) => void;
  onSearch: (value: string) => void;
  placeholder?: string;
  style?: React.CSSProperties;
}) {
  const { t } = i18n;
  const { token } = theme.useToken();
  const [history, setHistory] = useState<string[]>(() => loadSearchHistory(storageKey));
  const [open, setOpen] = useState(false);
  const inputRef = useRef<InputRef>(null);

  // 存储键变化（理论上不变）时重读历史。
  useEffect(() => {
    setHistory(loadSearchHistory(storageKey));
  }, [storageKey]);

  // 实时搜索（不按回车）也记入历史：输入停顿 1.2 秒后记录。
  useEffect(() => {
    const word = value.trim();
    if (!word) return;
    const timer = setTimeout(() => {
      setHistory(pushSearchHistory(storageKey, word));
    }, 1200);
    return () => clearTimeout(timer);
  }, [value, storageKey]);

  const submit = (word: string) => {
    setHistory(pushSearchHistory(storageKey, word));
    setOpen(false);
    onSearch(word);
  };

  const removeOne = (word: string) => {
    setHistory(removeSearchHistory(storageKey, word));
  };

  const clearAll = () => {
    clearSearchHistory(storageKey);
    setHistory([]);
    setOpen(false);
  };

  const items = history.map((word) => ({
    key: word,
    label: (
      <div
        style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, minWidth: 200 }}
        onMouseDown={(e) => e.preventDefault()}
      >
        <Typography.Text
          style={{ fontSize: 13, cursor: "pointer", flex: 1 }}
          onClick={() => submit(word)}
        >
          {word}
        </Typography.Text>
        <CloseOutlined
          style={{ fontSize: 10, color: token.colorTextTertiary }}
          onClick={(e) => {
            e.stopPropagation();
            removeOne(word);
          }}
        />
      </div>
    ),
  }));

  return (
    <Dropdown
      trigger={[]}
      open={open && history.length > 0}
      menu={{ items }}
      dropdownRender={(menu) => (
        <div
          style={{
            background: token.colorBgElevated,
            color: token.colorText,
            borderRadius: token.borderRadiusLG,
            boxShadow: token.boxShadowSecondary,
          }}
          onMouseDown={(e) => e.preventDefault()}
        >
          <div style={{ padding: "6px 12px", fontSize: 12, color: token.colorTextTertiary }}>
            {t("search.recent")}
          </div>
          {menu}
          <div
            style={{
              borderTop: `1px solid ${token.colorSplit}`,
              padding: "6px 12px",
              cursor: "pointer",
            }}
            onClick={clearAll}
          >
            <Space size={6}>
              <CloseOutlined style={{ fontSize: 10 }} />
              <Typography.Text style={{ fontSize: 12 }}>{t("search.clearHistory")}</Typography.Text>
            </Space>
          </div>
        </div>
      )}
    >
      <Input
        ref={inputRef}
        value={value}
        allowClear
        prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
        placeholder={placeholder}
        style={style}
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => {
          setHistory(loadSearchHistory(storageKey));
          setOpen(true);
        }}
        onBlur={() => setTimeout(() => setOpen(false), 150)}
        onPressEnter={(e) => submit((e.target as HTMLInputElement).value)}
      />
    </Dropdown>
  );
}
