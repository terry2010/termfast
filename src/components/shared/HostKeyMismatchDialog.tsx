// HostKeyMismatchDialog — shown when SSH server's host key has changed.
// Displays old vs new fingerprint and lets user accept or reject.

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "@/components/ui/Modal";
import { ipcInvoke } from "@/hooks/useIpc";
import { toast } from "sonner";

interface PendingMismatch {
  serverId: string;
  serverName: string;
  expected: string;
  actual: string;
}

export function HostKeyMismatchDialog() {
  const { t } = useTranslation();
  const [pending, setPending] = useState<PendingMismatch | null>(null);
  const [accepting, setAccepting] = useState(false);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as PendingMismatch;
      setPending(detail);
    };
    window.addEventListener("hostkey-mismatch", handler);
    return () => window.removeEventListener("hostkey-mismatch", handler);
  }, []);

  if (!pending) return null;

  const handleAccept = async () => {
    setAccepting(true);
    try {
      await ipcInvoke("ipc_accept_host_key", {
        serverId: pending.serverId,
        fingerprint: pending.actual,
      });
      toast.success(t("hostkey.accepted"));
      setPending(null);
    } catch (e) {
      toast.error(t("hostkey.accept_failed"), { description: String(e) });
    } finally {
      setAccepting(false);
    }
  };

  const handleCancel = () => {
    setPending(null);
  };

  return (
    <Modal
      title={t("hostkey.mismatch_title")}
      onClose={handleCancel}
      maxWidth="max-w-lg"
      footer={
        <>
          <button
            className="px-4 py-2 text-sm rounded-lg text-gray-600 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-[#2C2C2E] transition-colors"
            onClick={handleCancel}
            disabled={accepting}
          >
            {t("common.cancel")}
          </button>
          <button
            className="px-4 py-2 text-sm rounded-lg bg-red-600 text-white hover:bg-red-700 transition-colors disabled:opacity-50"
            onClick={handleAccept}
            disabled={accepting}
          >
            {accepting ? t("common.processing") : t("hostkey.accept_new_key")}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <p className="text-sm text-gray-600 dark:text-gray-400">
          {t("hostkey.mismatch_warning", { name: pending.serverName })}
        </p>
        <div className="space-y-2">
          <div className="rounded-lg bg-red-50 dark:bg-red-900/20 p-3">
            <p className="text-xs font-medium text-red-600 dark:text-red-400 mb-1">
              {t("hostkey.expected")}
            </p>
            <p className="text-xs font-mono text-red-700 dark:text-red-300 break-all">
              {pending.expected}
            </p>
          </div>
          <div className="rounded-lg bg-amber-50 dark:bg-amber-900/20 p-3">
            <p className="text-xs font-medium text-amber-600 dark:text-amber-400 mb-1">
              {t("hostkey.actual")}
            </p>
            <p className="text-xs font-mono text-amber-700 dark:text-amber-300 break-all">
              {pending.actual}
            </p>
          </div>
        </div>
        <p className="text-xs text-gray-500 dark:text-gray-500">
          {t("hostkey.mismatch_hint")}
        </p>
      </div>
    </Modal>
  );
}
