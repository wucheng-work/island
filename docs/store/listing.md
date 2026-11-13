# Microsoft Store listing: 灵动岛

Draft text for the customized build. No Store upload or product reservation is performed by editing this file. The package comes from the `store` workflow (artifact `oyenza-studio-msix`); existing package identity and compatibility filenames remain unchanged.

## Before the first upload

1. Create the developer account at [storedeveloper.microsoft.com](https://storedeveloper.microsoft.com).
2. Reserve the display name **灵动岛** in Partner Center.
3. Check **Product identity** against `tools/make_msix.ps1`: `Package/Identity/Name`, `Package/Identity/Publisher` and `Package/Properties/PublisherDisplayName`. Keep the existing identity when updating an installed package.
4. Review the built package, screenshots and privacy policy before submitting it.

## Product setup

| Field | Value |
|---|---|
| Pricing | Free |
| Markets | All |
| Category | Productivity |
| Website | https://github.com/lucacadalora/oyenza-studio |
| Support contact | https://github.com/lucacadalora/oyenza-studio/issues |
| Privacy policy URL | https://github.com/lucacadalora/oyenza-studio/blob/main/PRIVACY.md |
| Copyright | © 2026 Jatevo Labs |

## Store listing (English)

**Short description**

A small island at the top of your screen for music, a calendar with today's tasks, a focus timer, local files and optional system monitoring.

**Description**

灵动岛 puts a small island at the top of your screen. It shows your music, a focus countdown and system pop-ups, then opens into quick pages when you point at it.

The pages keep everyday actions close:

- Music: playback controls for the current player, album art and synced lyrics from supported player services.
- Today: a calendar on the left and today's tasks on the right. Unfinished tasks from earlier days stay visible until completed. Add, complete, delete and undo tasks from the same page.
- Shelf: drop in local files or folders, then open, reveal or drag them back out. The originals stay where they are.
- Focus: a Pomodoro timer that keeps counting while you work.
- System: CPU, graphics, memory, drives, network and battery readings. Enable or disable System monitoring in Settings.

Choose Classic, Oyen, Glass, Pill or Diorama, change the island's width and position, and decide which pages stay in its tab row. Volume, charging and Bluetooth pop-ups can each be switched on or off.

The app is native Rust and uses no web view. Core local features need no account. Tasks, file references and settings are saved on your computer.

**What's new in this customized build**

- The display name is 灵动岛.
- Today and Tasks share one calendar and to-do page.
- System monitoring is controlled from Settings.
- The add-on gallery has been removed.

**Product features** (one per line in Partner Center)

- Playback controls, album art and a live music activity
- Synced lyrics from supported current-player services
- A calendar beside today's tasks, including earlier unfinished tasks
- Add, complete, delete and undo tasks
- A local file shelf for files and folders
- A Pomodoro focus timer
- Optional system monitoring, enabled in Settings
- Volume, charging and Bluetooth headset pop-ups
- Five themes and adjustable island width and position
- Optional startup with Windows

**Search terms** (up to 7)

dynamic island, notch, lyrics, pomodoro, calendar, tasks, files

**Screenshots**

Desktop screenshots must be at least 1366 × 768. Use captures of the current build: Music, Today with the calendar and tasks, Shelf, Focus and System monitoring. Screenshots must show the merged Today page and current tab row.

## Age rating (IARC questionnaire)

Category: utility/productivity. No violence, sexual content, gambling or in-app purchases. Users cannot chat with each other through the app. Expected rating: 3+ / Everyone, subject to the certification questionnaire.

## Notes for certification

> 灵动岛 is a Win32 desktop app built with Rust and Direct2D. Point at the island at the top of the screen, or click it, to open it. Use a player exposed through Windows media controls to see playback controls; synced lyrics depend on the current player's supported service and may require an internet connection or an Apple Music token. Tasks, the local file shelf and Focus work without an account. Enable System monitoring from Settings. "Start with Windows" is the package's startup task (TaskId PoniStartup), which users can disable in the app menu or Windows startup settings.
>
> The optional Glass theme copies the region behind the island with DXGI Desktop Duplication. The copy stays in memory, is replaced by the next frame, and is never saved or sent. While Glass is active the island is excluded from screen capture. Capture stops while the island is hidden or the PC is locked.

**runFullTrust justification**

> The app needs full trust for its always-on-top island window, system volume and media session controls, system monitoring, and dragging local files to and from the island. File shelf entries store references to the original paths; removing an entry does not delete the original file.
