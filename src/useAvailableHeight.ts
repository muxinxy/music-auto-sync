import { useEffect, useRef, useState } from "react";

/**
 * 测量容器可用高度（ResizeObserver），供表格 `scroll.y` 使用：
 * 窗口缩小时表格滚动区随之收缩，分页始终留在容器内可见。
 */
export function useAvailableHeight(offset = 0): [React.RefObject<HTMLDivElement>, number] {
  const ref = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(480);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      const next = Math.max(160, Math.floor(rect.height - offset));
      setHeight(next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [offset]);

  return [ref, height];
}
