// ContextMenu — right-click context menu (§9.11)
import { useEffect, useRef, useState, type ReactNode } from "react";

export interface ContextMenuItem {
  label: string;
  icon?: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  separator?: false;
}

export interface ContextMenuSeparator {
  separator: true;
}

export type ContextMenuEntry = ContextMenuItem | ContextMenuSeparator;

interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuEntry[];
}

let globalSetMenu: ((state: ContextMenuState | null) => void) | null = null;

export function showContextMenu(e: React.MouseEvent, items: ContextMenuEntry[]) {
  e.preventDefault();
  e.stopPropagation();
  if (globalSetMenu) {
    globalSetMenu({ x: e.clientX, y: e.clientY, items });
  }
}

export function ContextMenuProvider({ children }: { children: ReactNode }) {
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    globalSetMenu = setMenu;
    return () => { globalSetMenu = null; };
  }, []);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setMenu(null);
      }
    };
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu(null);
    };
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleEsc);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleEsc);
    };
  }, [menu]);

  if (!menu) return <>{children}</>;

  // Adjust position to keep menu on screen
  const menuWidth = 200;
  const menuHeight = menu.items.length * 32 + 8;
  const x = Math.min(menu.x, window.innerWidth - menuWidth - 8);
  const y = Math.min(menu.y, window.innerHeight - menuHeight - 8);

  return (
    <>
      {children}
      <div
        ref={ref}
        className="fixed z-[100] min-w-[180px] py-1 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700"
        style={{ left: x, top: y }}
      >
        {menu.items.map((item, i) => {
          if ("separator" in item) {
            return <div key={i} className="h-px my-1 bg-gray-200 dark:bg-gray-700" />;
          }
          return (
            <button
              key={i}
              className={`w-full text-left px-3 py-1.5 text-sm flex items-center gap-2 ${
                item.disabled
                  ? "text-gray-400 cursor-not-allowed"
                  : item.danger
                  ? "text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30"
                  : "text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700"
              }`}
              disabled={item.disabled}
              onClick={() => {
                if (!item.disabled) {
                  item.onClick();
                  setMenu(null);
                }
              }}
            >
              {item.icon && <span className="w-4 text-center">{item.icon}</span>}
              <span>{item.label}</span>
            </button>
          );
        })}
      </div>
    </>
  );
}
