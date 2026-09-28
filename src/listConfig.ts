import type { PaginationProps, TablePaginationConfig } from "antd";

/**
 * 统一的分页/表格样式：所有列表与表格保持一致的每页条数、页大小切换与总数展示，
 * 信息密度统一、便于对照阅读。
 */
export const tablePagination: TablePaginationConfig = {
  pageSize: 20,
  showSizeChanger: true,
  pageSizeOptions: [20, 50, 100],
  size: "small",
  showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}`,
};

export const listPagination: PaginationProps = {
  ...tablePagination,
  size: "small",
};
