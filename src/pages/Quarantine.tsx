import { useCallback, useEffect, useMemo, useState } from "react";
import type { Key } from "react";
import {
  Button,
  Card,
  DatePicker,
  Input,
  Popconfirm,
  Select,
  Space,
  Table,
  Typography,
  message as antMessage,
} from "antd";
import { DeleteOutlined, UndoOutlined } from "@ant-design/icons";
import dayjs, { type Dayjs } from "dayjs";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { formatError } from "../errors";
import type { QuarantineItem } from "../types";
import SearchWithHistory from "../SearchWithHistory";
import { loadFilters, saveFilters } from "../filterMemory";
import { useAvailableHeight } from "../useAvailableHeight";
import { tablePagination } from "../listConfig";

const { RangePicker } = DatePicker;

/** 原路径的父目录（用于目录筛选）。 */
function parentDir(path: string): string {
  const idx = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return idx > 0 ? path.slice(0, idx) : path;
}

export default function QuarantinePage() {
  const { t } = useTranslation();
  const [items, setItems] = useState<QuarantineItem[]>([]);
  const [loading, setLoading] = useState(true);
  // 筛选条件本地记忆：切页面回来保持上次的筛选。
  const savedFilters = loadFilters("filters.quarantine", {
    search: "",
    dir: "",
    rangeStart: "",
    rangeEnd: "",
  });
  const [search, setSearch] = useState(savedFilters.search);
  const [dir, setDir] = useState<string | undefined>(savedFilters.dir || undefined);
  const [range, setRange] = useState<[Dayjs | null, Dayjs | null] | null>(
    savedFilters.rangeStart && savedFilters.rangeEnd
      ? [dayjs(savedFilters.rangeStart), dayjs(savedFilters.rangeEnd)]
      : null
  );
  useEffect(() => {
    saveFilters("filters.quarantine", {
      search,
      dir: dir ?? "",
      rangeStart: range?.[0]?.toISOString() ?? "",
      rangeEnd: range?.[1]?.toISOString() ?? "",
    });
  }, [search, dir, range]);

  // 表格滚动高度随容器测量：小窗口下分页仍可见。
  const [tableWrapRef, tableHeight] = useAvailableHeight(112);

  const clearFilters = () => {
    setSearch("");
    setDir(undefined);
    setRange(null);
  };
  const [selectedKeys, setSelectedKeys] = useState<Key[]>([]);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.listQuarantine());
    } catch (e) {
      antMessage.error(t("quarantine.loadFailed", { detail: formatError(e) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  // 目录筛选项：原路径父目录去重。
  const dirOptions = useMemo(() => {
    const set = new Set<string>();
    for (const item of items) set.add(parentDir(item.originalPath));
    return Array.from(set).sort().map((d) => ({ value: d, label: d }));
  }, [items]);

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return items.filter((item) => {
      if (
        needle &&
        !item.fileName.toLowerCase().includes(needle) &&
        !item.originalPath.toLowerCase().includes(needle)
      ) {
        return false;
      }
      if (dir && parentDir(item.originalPath) !== dir) return false;
      if (range?.[0] && range?.[1]) {
        const time = dayjs(item.quarantinedAt, "YYYY-MM-DD HH:mm:ss");
        if (time.isBefore(range[0].startOf("day")) || time.isAfter(range[1].endOf("day"))) {
          return false;
        }
      }
      return true;
    });
  }, [items, search, dir, range]);

  const sourceName = (item: QuarantineItem) =>
    item.playlistName === "cleanup" ? t("quarantine.cleanupSource") : item.playlistName;

  const batchRestore = async () => {
    try {
      const count = await api.quarantineBatchRestore(selectedKeys.map(Number));
      antMessage.success(t("quarantine.batchRestored", { count }));
      setSelectedKeys([]);
      load();
    } catch (e) {
      antMessage.error(t("quarantine.restoreFailed", { detail: formatError(e) }));
    }
  };

  const batchDelete = async () => {
    try {
      const count = await api.quarantineBatchDelete(selectedKeys.map(Number));
      antMessage.success(t("quarantine.batchDeleted", { count }));
      setSelectedKeys([]);
      load();
    } catch (e) {
      antMessage.error(t("quarantine.deleteFailed", { detail: formatError(e) }));
    }
  };

  return (
    <div style={{ padding: 24, height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }}>
      <Card
        title={t("quarantine.title")}
        extra={<Typography.Text type="secondary">{t("quarantine.extra")}</Typography.Text>}
        style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}
        styles={{
          body: {
            padding: 0,
            flex: 1,
            minHeight: 0,
            overflow: "hidden",
            display: "flex",
            flexDirection: "column",
          },
        }}
      >
        <Space wrap style={{ padding: "16px 24px 0 24px" }}>
          <SearchWithHistory
            storageKey="search.quarantine"
            value={search}
            onChange={setSearch}
            onSearch={setSearch}
            placeholder={t("quarantine.searchPlaceholder")}
            style={{ width: 260 }}
          />
          <Select
            allowClear
            placeholder={t("quarantine.filterDir")}
            style={{ minWidth: 220, maxWidth: 360 }}
            value={dir}
            onChange={(value) => setDir(value)}
            options={dirOptions}
            showSearch
            optionFilterProp="label"
          />
          <RangePicker
            showTime
            value={range}
            onChange={(dates) => setRange(dates as [Dayjs | null, Dayjs | null] | null)}
            placeholder={[t("quarantine.dateFrom"), t("quarantine.dateTo")]}
          />
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {t("quarantine.shown", { shown: filtered.length, total: items.length })}
          </Typography.Text>
          <Button size="small" onClick={clearFilters}>
            {t("filters.clear")}
          </Button>
        </Space>
        <Space wrap style={{ padding: "8px 24px 0 24px" }}>
          {selectedKeys.length > 0 ? (
            <>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {t("quarantine.selectedCount", { count: selectedKeys.length })}
              </Typography.Text>
              <Button size="small" icon={<UndoOutlined />} onClick={batchRestore}>
                {t("quarantine.batchRestore", { count: selectedKeys.length })}
              </Button>
              <Popconfirm
                title={t("quarantine.batchDeleteConfirm", { count: selectedKeys.length })}
                okText={t("quarantine.confirmOk")}
                cancelText={t("quarantine.confirmCancel")}
                okButtonProps={{ danger: true }}
                onConfirm={batchDelete}
              >
                <Button size="small" danger icon={<DeleteOutlined />}>
                  {t("quarantine.batchDelete", { count: selectedKeys.length })}
                </Button>
              </Popconfirm>
              <Button size="small" type="link" onClick={() => setSelectedKeys([])}>
                {t("quarantine.clearSelection")}
              </Button>
            </>
          ) : (
            filtered.length > 0 && (
              <Button size="small" type="link" onClick={() => setSelectedKeys(filtered.map((i) => i.id))}>
                {t("quarantine.selectAllFiltered", { count: filtered.length })}
              </Button>
            )
          )}
        </Space>
        <div ref={tableWrapRef} style={{ flex: 1, minHeight: 0, marginTop: 8 }}>
        <Table<QuarantineItem>
          style={{ marginTop: 8 }}
          rowKey="id"
          loading={loading}
          dataSource={filtered}
          locale={{ emptyText: t("quarantine.empty") }}
          scroll={{ y: tableHeight }}
          pagination={tablePagination}
          rowSelection={{
            selectedRowKeys: selectedKeys,
            onChange: (keys) => setSelectedKeys(keys),
          }}
          columns={[
            {
              title: t("quarantine.colFile"),
              dataIndex: "fileName",
              ellipsis: true,
              sorter: (a, b) => a.fileName.localeCompare(b.fileName, "zh"),
            },
            {
              title: t("quarantine.colSource"),
              dataIndex: "playlistName",
              width: 140,
              ellipsis: true,
              render: (_, item) => sourceName(item),
            },
            {
              title: t("quarantine.colPath"),
              dataIndex: "originalPath",
              ellipsis: true,
              responsive: ["lg"],
            },
            {
              title: t("quarantine.colTime"),
              dataIndex: "quarantinedAt",
              width: 180,
              sorter: (a, b) => a.quarantinedAt.localeCompare(b.quarantinedAt),
              defaultSortOrder: "descend",
            },
            {
              title: t("quarantine.colActions"),
              key: "actions",
              width: 200,
              render: (_, item) => (
                <Space>
                  <Button
                    size="small"
                    icon={<UndoOutlined />}
                    onClick={async () => {
                      try {
                        await api.restoreQuarantine(item.id);
                        antMessage.success(t("quarantine.restored"));
                        load();
                      } catch (e) {
                        antMessage.error(t("quarantine.restoreFailed", { detail: formatError(e) }));
                      }
                    }}
                  >
                    {t("quarantine.restore")}
                  </Button>
                  <Popconfirm
                    title={t("quarantine.confirmTitle")}
                    description={t("quarantine.confirmDesc")}
                    okText={t("quarantine.confirmOk")}
                    cancelText={t("quarantine.confirmCancel")}
                    okButtonProps={{ danger: true }}
                    onConfirm={async () => {
                      try {
                        await api.deleteQuarantine(item.id);
                        antMessage.success(t("quarantine.deleted"));
                        load();
                      } catch (e) {
                        antMessage.error(t("quarantine.deleteFailed", { detail: formatError(e) }));
                      }
                    }}
                  >
                    <Button size="small" danger icon={<DeleteOutlined />}>
                      {t("quarantine.delete")}
                    </Button>
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
        </div>
      </Card>
    </div>
  );
}
