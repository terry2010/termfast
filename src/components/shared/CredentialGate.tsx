// CredentialGate — shows unlock/setup/migration screen before main app.
// Wraps the main app content and gates access until credentials are unlocked.

import { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { ipcInvoke } from "@/hooks/useIpc";

type CredentialStatus =
  | "needs_setup"
  | "needs_migration"
  | "locked"
  | "unlocked";

interface CredentialGateProps {
  children: React.ReactNode;
}

export function CredentialGate({ children }: CredentialGateProps) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<CredentialStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const checkStatus = useCallback(async () => {
    try {
      const s = await ipcInvoke<CredentialStatus>("ipc_credential_status");
      setStatus(s);
      if (s === "locked") {
        // Try cached unlock (OS keychain) first.
        const ok = await ipcInvoke<boolean>("ipc_try_cached_unlock");
        if (ok) {
          setStatus("unlocked");
        }
      }
    } catch (e) {
      // If credential IPC is not available (e.g. older backend), skip gate.
      console.error("[CredentialGate] status check failed:", e);
      setStatus("unlocked");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    checkStatus();
  }, [checkStatus]);

  const handleSetup = useCallback(
    async (password: string) => {
      setLoading(true);
      setError(null);
      try {
        await ipcInvoke("ipc_initialize_credentials", {
          masterPassword: password,
        });
        setStatus("unlocked");
      } catch (e: any) {
        setError(e?.message || String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const handleUnlock = useCallback(
    async (password: string) => {
      setLoading(true);
      setError(null);
      try {
        await ipcInvoke("ipc_unlock_credentials", {
          masterPassword: password,
        });
        setStatus("unlocked");
      } catch (e: any) {
        setError(e?.message || String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const handleMigrate = useCallback(
    async (password: string) => {
      setLoading(true);
      setError(null);
      try {
        await ipcInvoke("ipc_migrate_credentials", {
          masterPassword: password,
        });
        setStatus("unlocked");
      } catch (e: any) {
        setError(e?.message || String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  const handleSkipMigration = useCallback(() => {
    // User chose not to migrate — the encrypted store cannot be unlocked
    // (there's no master password yet). Show the locked state so the user
    // understands credentials are inaccessible. They can restart the app
    // to see the migration prompt again.
    setStatus("locked");
    setError(t("credentials.migration_skipped"));
  }, [t]);

  if (loading && status === null) {
    return (
      <div className="flex items-center justify-center h-screen bg-white dark:bg-[#121212]">
        <div className="text-gray-500 dark:text-gray-400 text-sm">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  if (status === "needs_setup") {
    return (
      <CredentialSetupScreen
        onSetup={handleSetup}
        loading={loading}
        error={error}
      />
    );
  }

  if (status === "needs_migration") {
    return (
      <CredentialMigrationDialog
        onMigrate={handleMigrate}
        onSkip={handleSkipMigration}
        loading={loading}
        error={error}
      />
    );
  }

  if (status === "locked") {
    return (
      <CredentialUnlockScreen
        onUnlock={handleUnlock}
        loading={loading}
        error={error}
      />
    );
  }

  // unlocked or null (fallback) — show main app
  return <>{children}</>;
}

// === SECTION 1 END ===

function CredentialSetupScreen({
  onSetup,
  loading,
  error,
}: {
  onSetup: (password: string) => void;
  loading: boolean;
  error: string | null;
}) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");

  const canSubmit =
    password.length >= 4 &&
    password === confirm &&
    !loading;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (canSubmit) onSetup(password);
  };

  return (
    <div className="flex items-center justify-center h-screen bg-white dark:bg-[#121212]">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm space-y-4 p-8 rounded-2xl bg-gray-50 dark:bg-[#1E1E1E] shadow-lg"
      >
        <div className="text-center space-y-2">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {t("credentials.setup_title")}
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {t("credentials.setup_description")}
          </p>
        </div>
        <div className="space-y-3">
          <input
            type="password"
            placeholder={t("credentials.master_password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            autoFocus
          />
          <input
            type="password"
            placeholder={t("credentials.confirm_password")}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
          />
          {password && confirm && password !== confirm && (
            <p className="text-xs text-red-500">
              {t("credentials.password_mismatch")}
            </p>
          )}
          {password && password.length < 4 && (
            <p className="text-xs text-gray-400">
              {t("credentials.password_too_short")}
            </p>
          )}
        </div>
        {error && (
          <p className="text-sm text-red-500 text-center">{error}</p>
        )}
        <button
          type="submit"
          disabled={!canSubmit}
          className="w-full py-2.5 rounded-lg bg-blue-600 text-white font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {loading ? t("common.loading") : t("credentials.setup_button")}
        </button>
      </form>
    </div>
  );
}

// === SECTION 2 END ===

function CredentialUnlockScreen({
  onUnlock,
  loading,
  error,
}: {
  onUnlock: (password: string) => void;
  loading: boolean;
  error: string | null;
}) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (password && !loading) onUnlock(password);
  };

  return (
    <div className="flex items-center justify-center h-screen bg-white dark:bg-[#121212]">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm space-y-4 p-8 rounded-2xl bg-gray-50 dark:bg-[#1E1E1E] shadow-lg"
      >
        <div className="text-center space-y-2">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {t("credentials.unlock_title")}
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {t("credentials.unlock_description")}
          </p>
        </div>
        <input
          type="password"
          placeholder={t("credentials.master_password")}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
          autoFocus
        />
        {error && (
          <p className="text-sm text-red-500 text-center">{error}</p>
        )}
        <button
          type="submit"
          disabled={!password || loading}
          className="w-full py-2.5 rounded-lg bg-blue-600 text-white font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {loading ? t("common.loading") : t("credentials.unlock_button")}
        </button>
      </form>
    </div>
  );
}

// === SECTION 3 END ===

function CredentialMigrationDialog({
  onMigrate,
  onSkip,
  loading,
  error,
}: {
  onMigrate: (password: string) => void;
  onSkip: () => void;
  loading: boolean;
  error: string | null;
}) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");

  const canSubmit =
    password.length >= 4 &&
    password === confirm &&
    !loading;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (canSubmit) onMigrate(password);
  };

  return (
    <div className="flex items-center justify-center h-screen bg-white dark:bg-[#121212]">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-md space-y-4 p-8 rounded-2xl bg-gray-50 dark:bg-[#1E1E1E] shadow-lg"
      >
        <div className="text-center space-y-2">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {t("credentials.migration_title")}
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            {t("credentials.migration_description")}
          </p>
        </div>
        <div className="space-y-3">
          <input
            type="password"
            placeholder={t("credentials.master_password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
            autoFocus
          />
          <input
            type="password"
            placeholder={t("credentials.confirm_password")}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            className="w-full px-4 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-[#2A2A2A] text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-blue-500 outline-none"
          />
          {password && confirm && password !== confirm && (
            <p className="text-xs text-red-500">
              {t("credentials.password_mismatch")}
            </p>
          )}
        </div>
        {error && (
          <p className="text-sm text-red-500 text-center">{error}</p>
        )}
        <div className="flex gap-3">
          <button
            type="button"
            onClick={onSkip}
            disabled={loading}
            className="flex-1 py-2.5 rounded-lg border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 font-medium hover:bg-gray-100 dark:hover:bg-[#2A2A2A] disabled:opacity-50 transition-colors"
          >
            {t("credentials.migration_skip")}
          </button>
          <button
            type="submit"
            disabled={!canSubmit}
            className="flex-1 py-2.5 rounded-lg bg-blue-600 text-white font-medium hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {loading ? t("common.loading") : t("credentials.migration_button")}
          </button>
        </div>
      </form>
    </div>
  );
}
