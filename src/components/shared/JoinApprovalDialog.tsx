// D1/D2: Join approval dialog — shown when desktop receives join_batch_pending notification.
// Displays batch info (flow_type, target_members, initiator) and lets user approve or reject.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../ui/Modal";
import { getBatchInfo, approveJoin, type BatchInfo } from "@/lib/joinNetwork";

interface JoinApprovalDialogProps {
  batchId: string;
  token: string;
  approverUserId: number;
  onClose: () => void;
  onApproved?: (batchId: string) => void;
}

export function JoinApprovalDialog({
  batchId,
  token,
  approverUserId,
  onClose,
  onApproved,
}: JoinApprovalDialogProps) {
  const { t } = useTranslation();
  const [batchInfo, setBatchInfo] = useState<BatchInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [approving, setApproving] = useState(false);
  const [approveResult, setApproveResult] = useState<"success" | "error" | null>(null);

  // Fetch batch info on mount
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const info = await getBatchInfo({ batchId, token });
        if (!cancelled) {
          setBatchInfo(info);
          setLoading(false);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Failed to load batch info");
          setLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [batchId, token]);

  const handleApprove = async () => {
    if (!batchInfo) return;
    setApproving(true);
    setError(null);
    try {
      await approveJoin({
        batchId,
        token,
        approverUserId,
        serverNonce: batchInfo.server_nonce,
        batchContentHash: batchInfo.batch_content_hash,
      });
      setApproveResult("success");
      onApproved?.(batchId);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Approval failed");
      setApproveResult("error");
    } finally {
      setApproving(false);
    }
  };

  const flowTypeLabel = (flowType: string): string => {
    if (flowType === "merge") return t("joinApproval.flowMerge", "网络合并");
    if (flowType === "new_device") return t("joinApproval.flowNewDevice", "新设备加入");
    return flowType;
  };

  const handleClose = () => {
    if (approving) return; // Prevent closing while approving
    onClose();
  };

  return (
    <Modal
      title={t("joinApproval.title", "互联提案批准")}
      onClose={handleClose}
      maxWidth="max-w-lg"
      footer={
        approveResult === "success" ? (
          <button
            className="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 transition-colors"
            onClick={onClose}
          >
            {t("common.close", "关闭")}
          </button>
        ) : (
          <>
            <button
              className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-[#2C2C2E] rounded-lg hover:bg-gray-200 dark:hover:bg-[#3A3A3C] transition-colors"
              onClick={handleClose}
              disabled={approving}
            >
              {t("joinApproval.reject", "拒绝")}
            </button>
            <button
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={handleApprove}
              disabled={approving || !batchInfo}
            >
              {approving
                ? t("joinApproval.approving", "批准中...")
                : t("joinApproval.approve", "批准")}
            </button>
          </>
        )
      }
    >
      {loading && (
        <div className="flex items-center justify-center py-8">
          <div className="animate-spin h-6 w-6 border-2 border-blue-600 border-t-transparent rounded-full" />
        </div>
      )}

      {error && approveResult !== "success" && (
        <div className="text-sm text-red-600 dark:text-red-400 py-4">
          {error}
        </div>
      )}

      {approveResult === "success" && (
        <div className="py-4 text-sm text-green-600 dark:text-green-400">
          {t("joinApproval.approved", "已批准。等待其他设备签名...")}
        </div>
      )}

      {batchInfo && approveResult !== "success" && (
        <div className="space-y-4">
          {/* Flow type */}
          <div>
            <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">
              {t("joinApproval.flowType", "提案类型")}
            </div>
            <div className="text-sm font-medium text-gray-900 dark:text-gray-100">
              {flowTypeLabel(batchInfo.flow_type)}
            </div>
          </div>

          {/* Initiator */}
          <div>
            <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">
              {t("joinApproval.initiator", "发起者")}
            </div>
            <div className="text-sm text-gray-900 dark:text-gray-100">
              {t("joinApproval.userLabel", "用户")} #{batchInfo.initiator_user_id}
            </div>
          </div>

          {/* Target members */}
          <div>
            <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">
              {t("joinApproval.targetMembers", "目标成员")}
            </div>
            <ul className="space-y-1">
              {batchInfo.target_members.map((m) => (
                <li
                  key={`${m.user_id}-${m.device_id}`}
                  className="text-sm text-gray-900 dark:text-gray-100 flex items-center gap-2"
                >
                  <span className="inline-block w-2 h-2 rounded-full bg-blue-500" />
                  {m.device_name || m.device_id}{" "}
                  <span className="text-gray-400">
                    ({t("joinApproval.userLabel", "用户")} #{m.user_id})
                  </span>
                </li>
              ))}
            </ul>
          </div>

          {/* Approval progress */}
          <div>
            <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">
              {t("joinApproval.progress", "签名进度")}
            </div>
            <div className="text-sm text-gray-900 dark:text-gray-100">
              {batchInfo.received_approvals} / {batchInfo.required_approvals}
            </div>
          </div>

          {/* Expiry */}
          <div>
            <div className="text-xs text-gray-500 dark:text-gray-400 mb-1">
              {t("joinApproval.expiresAt", "过期时间")}
            </div>
            <div className="text-sm text-gray-900 dark:text-gray-100">
              {new Date(batchInfo.expires_at).toLocaleString()}
            </div>
          </div>
        </div>
      )}
    </Modal>
  );
}
