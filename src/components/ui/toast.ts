// Toast — unified toast notifications using sonner
// Usage: import { toast } from "@/components/ui/toast"
//   toast.success("Done")
//   toast.error("Failed")
//   toast.warning("Warning")
//   toast.info("Info")

import { toast as sonnerToast } from "sonner";

export type ToastLevel = "success" | "error" | "warning" | "info";

interface ToastOptions {
  description?: string;
  duration?: number;
}

export const toast = {
  success(message: string, opts?: ToastOptions) {
    sonnerToast.success(message, {
      description: opts?.description,
      duration: opts?.duration ?? 4000,
    });
  },

  error(message: string, opts?: ToastOptions) {
    sonnerToast.error(message, {
      description: opts?.description,
      duration: opts?.duration ?? 6000,
    });
  },

  warning(message: string, opts?: ToastOptions) {
    sonnerToast.warning(message, {
      description: opts?.description,
      duration: opts?.duration ?? 5000,
    });
  },

  info(message: string, opts?: ToastOptions) {
    sonnerToast.info(message, {
      description: opts?.description,
      duration: opts?.duration ?? 4000,
    });
  },
};
