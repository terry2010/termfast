// Modal — shared modal shell with consistent styling
// Provides: backdrop, centered card, header with title + close button, body, footer

import { useEffect, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  maxWidth?: string;
  zIndex?: string;
  bodyClassName?: string;
}

export function Modal({
  title,
  onClose,
  children,
  footer,
  maxWidth = "max-w-md",
  zIndex = "z-50",
  bodyClassName,
}: ModalProps) {
  const { t } = useTranslation();

  // ESC to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  return (
    <div
      className={`fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center ${zIndex} animate-[fadeIn_0.15s_ease-out]`}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`bg-white dark:bg-[#1E1E1E] rounded-xl shadow-2xl ${maxWidth} w-full mx-4 max-h-[85vh] overflow-hidden flex flex-col animate-[scaleIn_0.15s_ease-out] border border-gray-200/80 dark:border-white/[0.06]`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-white/[0.06]">
          <h2 className="text-base font-semibold text-gray-900 dark:text-gray-100">
            {title}
          </h2>
          <button
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors w-7 h-7 flex items-center justify-center rounded-md hover:bg-gray-100 dark:hover:bg-[#2C2C2E]"
            onClick={onClose}
            aria-label={t("common.close")}
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M1 1L13 13M13 1L1 13"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div
          className={`flex-1 overflow-y-auto px-5 py-4 ${bodyClassName ?? ""}`}
        >
          {children}
        </div>

        {/* Footer */}
        {footer && (
          <div className="px-5 py-3 border-t border-gray-100 dark:border-white/[0.06] flex justify-end gap-2 bg-[#FBFBFB] dark:bg-[#1E1E1E]">
            {footer}
          </div>
        )}
      </div>
    </div>
  );
}
