package com.termfast.app.service

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/**
 * Unit tests for the host-key fingerprint extraction logic used by
 * [SshVpnService] when a `HostKeyMismatch` error is reported by the Rust
 * backend.
 *
 * The Rust side (crates/core/src/ssh/client.rs) produces a detail string of
 * the form `expected: SHA256:xxx, got: SHA256:yyy` when both the known and
 * actual fingerprints are available. When the actual key is unavailable the
 * detail becomes `expected: SHA256:xxx, got: <unknown>`, and when neither is
 * available it becomes `host key verification failed`.
 *
 * The Android UI extracts the actual fingerprint using
 * `Regex("got:\\s*(SHA256:\\S+)").find(raw)?.groupValues?.get(1)` and must
 * only show the mismatch dialog when a usable SHA256 fingerprint is present.
 */
class HostKeyFingerprintParserTest {

    private val pattern = Regex("got:\\s*(SHA256:\\S+)")

    private fun extractActual(raw: String): String? =
        pattern.find(raw)?.groupValues?.get(1)

    @Test
    fun extracts_actual_fingerprint_from_full_detail() {
        val raw = "expected: SHA256:aaa, got: SHA256:bbb"
        assertEquals("SHA256:bbb", extractActual(raw))
    }

    @Test
    fun returns_null_when_actual_is_unknown() {
        val raw = "expected: SHA256:aaa, got: <unknown>"
        assertNull(extractActual(raw))
    }

    @Test
    fun returns_null_for_fallback_detail() {
        val raw = "host key verification failed"
        assertNull(extractActual(raw))
    }

    @Test
    fun returns_null_for_empty_string() {
        assertNull(extractActual(""))
    }

    @Test
    fun handles_realistic_sha256_fingerprint() {
        val fp = "SHA256:W0u9ZvQlM1xY5bJ9dCtQpPmN8gKqR2sF4uH7vXyZabc="
        val raw = "expected: SHA256:AAAA, got: $fp"
        assertEquals(fp, extractActual(raw))
    }

    @Test
    fun handles_multiline_detail() {
        val raw = "SSH connect to 1.2.3.4:22 failed\nexpected: SHA256:aaa, got: SHA256:bbb"
        assertEquals("SHA256:bbb", extractActual(raw))
    }

    @Test
    fun does_not_match_non_sha256_got_value() {
        val raw = "expected: SHA256:aaa, got: MD5:bbb"
        assertNull(extractActual(raw))
    }
}
