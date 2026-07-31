package com.termfast.app.ui

import android.graphics.Color as AndroidColor

/**
 * Terminal color scheme preset.
 * Colors are stored as ARGB Int for direct use with termlib's
 * `applyColorScheme(int[] ansiColors, int defaultForeground, int defaultBackground)`.
 */
data class TerminalTheme(
    val id: String,
    val name: String,
    val isDark: Boolean,
    val foreground: Int,      // ARGB
    val background: Int,      // ARGB
    val cursor: Int,          // ARGB
    val selectionBackground: Int, // ARGB
    val ansiColors: IntArray, // 16 colors (0-15): black, red, green, yellow, blue, magenta, cyan, white, brightBlack, ...
)

private fun hex(argb: String): Int = AndroidColor.parseColor(argb)

object TerminalThemes {

    val catppuccinMocha = TerminalTheme(
        id = "catppuccin-mocha",
        name = "Catppuccin Mocha",
        isDark = true,
        foreground = hex("#cdd6f4"),
        background = hex("#1e1e2e"),
        cursor = hex("#f5e0dc"),
        selectionBackground = hex("#585b70"),
        ansiColors = intArrayOf(
            hex("#45475a"), hex("#f38ba8"), hex("#a6e3a1"), hex("#f9e2af"),
            hex("#89b4fa"), hex("#f5c2e7"), hex("#94e2d5"), hex("#bac2de"),
            hex("#585b70"), hex("#f38ba8"), hex("#a6e3a1"), hex("#f9e2af"),
            hex("#89b4fa"), hex("#f5c2e7"), hex("#94e2d5"), hex("#a6adc8"),
        ),
    )

    val catppuccinLatte = TerminalTheme(
        id = "catppuccin-latte",
        name = "Catppuccin Latte",
        isDark = false,
        foreground = hex("#4c4f69"),
        background = hex("#eff1f5"),
        cursor = hex("#dc8a78"),
        selectionBackground = hex("#bcc0cc"),
        ansiColors = intArrayOf(
            hex("#5c5f77"), hex("#d20f39"), hex("#40a02b"), hex("#df8e1d"),
            hex("#1e66f5"), hex("#ea76cb"), hex("#179299"), hex("#acb0be"),
            hex("#6c6f85"), hex("#d20f39"), hex("#40a02b"), hex("#df8e1d"),
            hex("#1e66f5"), hex("#ea76cb"), hex("#179299"), hex("#bcc0cc"),
        ),
    )

    // === SECTION 1 END ===

    val dracula = TerminalTheme(
        id = "dracula",
        name = "Dracula",
        isDark = true,
        foreground = hex("#f8f8f2"),
        background = hex("#282a36"),
        cursor = hex("#bd93f9"),
        selectionBackground = hex("#44475a"),
        ansiColors = intArrayOf(
            hex("#21222c"), hex("#ff5555"), hex("#50fa7b"), hex("#f1fa8c"),
            hex("#bd93f9"), hex("#ff79c6"), hex("#8be9fd"), hex("#f8f8f2"),
            hex("#6272a4"), hex("#ff6e6e"), hex("#69ff94"), hex("#ffffa5"),
            hex("#d6acff"), hex("#ff92df"), hex("#a4ffff"), hex("#ffffff"),
        ),
    )

    val oneDark = TerminalTheme(
        id = "one-dark",
        name = "One Dark",
        isDark = true,
        foreground = hex("#abb2bf"),
        background = hex("#282c34"),
        cursor = hex("#528bff"),
        selectionBackground = hex("#3e4451"),
        ansiColors = intArrayOf(
            hex("#282c34"), hex("#e06c75"), hex("#98c379"), hex("#e5c07b"),
            hex("#61afef"), hex("#c678dd"), hex("#56b6c2"), hex("#abb2bf"),
            hex("#5c6370"), hex("#e06c75"), hex("#98c379"), hex("#e5c07b"),
            hex("#61afef"), hex("#c678dd"), hex("#56b6c2"), hex("#ffffff"),
        ),
    )

    // === SECTION 2 END ===

    val solarizedDark = TerminalTheme(
        id = "solarized-dark",
        name = "Solarized Dark",
        isDark = true,
        foreground = hex("#839496"),
        background = hex("#002b36"),
        cursor = hex("#93a1a1"),
        selectionBackground = hex("#073642"),
        ansiColors = intArrayOf(
            hex("#073642"), hex("#dc322f"), hex("#859900"), hex("#b58900"),
            hex("#268bd2"), hex("#d33682"), hex("#2aa198"), hex("#eee8d5"),
            hex("#586e75"), hex("#cb4b16"), hex("#586e75"), hex("#657b83"),
            hex("#839496"), hex("#6c71c4"), hex("#93a1a1"), hex("#fdf6e3"),
        ),
    )

    val solarizedLight = TerminalTheme(
        id = "solarized-light",
        name = "Solarized Light",
        isDark = false,
        foreground = hex("#657b83"),
        background = hex("#fdf6e3"),
        cursor = hex("#586e75"),
        selectionBackground = hex("#eee8d5"),
        ansiColors = intArrayOf(
            hex("#073642"), hex("#dc322f"), hex("#859900"), hex("#b58900"),
            hex("#268bd2"), hex("#d33682"), hex("#2aa198"), hex("#eee8d5"),
            hex("#586e75"), hex("#cb4b16"), hex("#586e75"), hex("#657b83"),
            hex("#839496"), hex("#6c71c4"), hex("#93a1a1"), hex("#fdf6e3"),
        ),
    )

    // === SECTION 3 END ===

    val gruvboxDark = TerminalTheme(
        id = "gruvbox-dark",
        name = "Gruvbox Dark",
        isDark = true,
        foreground = hex("#ebdbb2"),
        background = hex("#282828"),
        cursor = hex("#ebdbb2"),
        selectionBackground = hex("#504945"),
        ansiColors = intArrayOf(
            hex("#282828"), hex("#cc241d"), hex("#98971a"), hex("#d79921"),
            hex("#458588"), hex("#b16286"), hex("#689d6a"), hex("#a89984"),
            hex("#928374"), hex("#fb4934"), hex("#b8bb26"), hex("#fabd2f"),
            hex("#83a598"), hex("#d3869b"), hex("#8ec07c"), hex("#ebdbb2"),
        ),
    )

    val nord = TerminalTheme(
        id = "nord",
        name = "Nord",
        isDark = true,
        foreground = hex("#d8dee9"),
        background = hex("#2e3440"),
        cursor = hex("#d8dee9"),
        selectionBackground = hex("#434c5e"),
        ansiColors = intArrayOf(
            hex("#3b4252"), hex("#bf616a"), hex("#a3be8c"), hex("#ebcb8b"),
            hex("#81a1c1"), hex("#b48ead"), hex("#88c0d0"), hex("#e5e9f0"),
            hex("#4c566a"), hex("#bf616a"), hex("#a3be8c"), hex("#ebcb8b"),
            hex("#81a1c1"), hex("#b48ead"), hex("#8fbcbb"), hex("#eceff4"),
        ),
    )

    // === SECTION 4 END ===

    val tokyoNight = TerminalTheme(
        id = "tokyo-night",
        name = "Tokyo Night",
        isDark = true,
        foreground = hex("#a9b1d6"),
        background = hex("#1a1b26"),
        cursor = hex("#c0caf5"),
        selectionBackground = hex("#33467c"),
        ansiColors = intArrayOf(
            hex("#15161e"), hex("#f7768e"), hex("#9ece6a"), hex("#e0af68"),
            hex("#7aa2f7"), hex("#bb9af7"), hex("#7dcfff"), hex("#a9b1d6"),
            hex("#414868"), hex("#f7768e"), hex("#9ece6a"), hex("#e0af68"),
            hex("#7aa2f7"), hex("#bb9af7"), hex("#7dcfff"), hex("#c0caf5"),
        ),
    )

    val githubDark = TerminalTheme(
        id = "github-dark",
        name = "GitHub Dark",
        isDark = true,
        foreground = hex("#c9d1d9"),
        background = hex("#0d1117"),
        cursor = hex("#73b7f0"),
        selectionBackground = hex("#264f78"),
        ansiColors = intArrayOf(
            hex("#484f58"), hex("#ff7b72"), hex("#3fb950"), hex("#d29922"),
            hex("#58a6ff"), hex("#bc8cff"), hex("#39c5cf"), hex("#b1bac4"),
            hex("#6e7681"), hex("#ffa198"), hex("#56d364"), hex("#e3b341"),
            hex("#79c0ff"), hex("#d2a8ff"), hex("#56d4dd"), hex("#f0f6fc"),
        ),
    )

    // === SECTION 5 END ===

    val all = listOf(
        catppuccinMocha,
        catppuccinLatte,
        dracula,
        oneDark,
        solarizedDark,
        solarizedLight,
        gruvboxDark,
        nord,
        tokyoNight,
        githubDark,
    )

    private val byIdMap = all.associateBy { it.id }

    /** Get theme by ID, falling back to Catppuccin Mocha. */
    fun byId(id: String): TerminalTheme = byIdMap[id] ?: catppuccinMocha
}
