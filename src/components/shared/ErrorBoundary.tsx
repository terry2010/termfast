// ErrorBoundary — catches render errors to prevent white screen

import { Component, type ReactNode } from "react";
import i18n from "@/i18n/config";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("ErrorBoundary caught:", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="fixed inset-0 flex items-center justify-center p-8 bg-white dark:bg-[#1E1E1E]">
          <div className="max-w-md space-y-4">
            <h1 className="text-xl font-bold text-red-600">
              {i18n.t("error.something_wrong")}
            </h1>
            <pre className="text-sm text-gray-700 dark:text-gray-300 whitespace-pre-wrap break-all">
              {this.state.error?.message || i18n.t("common.unknown_error")}
            </pre>
            <pre className="text-xs text-gray-500 whitespace-pre-wrap break-all max-h-40 overflow-auto">
              {this.state.error?.stack}
            </pre>
            <button
              className="px-4 py-2 rounded bg-blue-500 text-white hover:bg-blue-600"
              onClick={() => this.setState({ hasError: false, error: null })}
            >
              {i18n.t("error.retry")}
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

// === SECTION 1 END ===
