// D7: Hook to check trust level of mobile pairings.
// Used to conditionally hide interconnect terminal UI for "local_only" sessions.

import { useState, useEffect } from "react";
import { fetchPairedDevices } from "@/lib/pairing";

/**
 * Checks if any mobile pairing has trust_level=local_only.
 * If so, the interconnect terminal UI should be hidden (D7: UX-level control).
 *
 * Note: This is a UX-level control only. It does NOT prevent a shell attacker
 * from accessing interconnect functionality (see design doc §4.4).
 */
export function useTrustLevelCheck(token: string | null): {
  hasLocalOnlyPairing: boolean;
  loading: boolean;
} {
  const [hasLocalOnly, setHasLocalOnly] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!token) {
      setHasLocalOnly(false);
      setLoading(false);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const devices = await fetchPairedDevices(token);
        if (cancelled) return;
        // Check if any mobile pairing has trust_level=local_only
        const hasLocal = devices.some(
          (d: any) => d.pairing_type === "mobile" && d.trust_level === "local_only"
        );
        setHasLocalOnly(hasLocal);
      } catch {
        if (!cancelled) setHasLocalOnly(false);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [token]);

  return { hasLocalOnlyPairing: hasLocalOnly, loading };
}
