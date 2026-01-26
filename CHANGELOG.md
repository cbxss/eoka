# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2025-01-26

### Added

- Initial release of eoka stealth browser automation library
- Custom CDP transport with built-in command filtering (blocks detectable commands like `Runtime.enable`)
- 15 JavaScript evasion scripts:
  - WebDriver property interception via Proxy
  - Navigator plugins/mimeTypes spoofing
  - Chrome runtime properties
  - Permissions API consistency fixes
  - Battery API on Navigator.prototype
  - WebGL vendor/renderer masking
  - Canvas noise injection
  - Audio fingerprint protection
  - iframe contentWindow fixes
  - Broken image dimension hiding
  - CDP property cleanup
  - WebRTC IP leak prevention
  - Speech synthesis voices spoofing
  - Media devices enumeration
  - Bluetooth API presence
- Human simulation with Bezier curve mouse movements and variable typing delays
- Chrome binary patching to remove automation strings (`$cdc_`, `webdriver`)
- Fingerprint generation (realistic User-Agent, screen dimensions)
- HTTP request capture via CDP Network domain with event streaming (`NetworkWatcher`)
- Screenshot capture with optional annotations (`annotate` feature)
- Session/cookie export for persistence
- GitHub Actions CI workflow (test, clippy, fmt, docs)
- 15 integration tests for CDP commands (browser launch, navigation, screenshots, etc.)

### Features

- `default` - Core functionality
- `annotate` - Screenshot annotations with numbered boxes on interactive elements

### Detection Test Results

- bot.sannysoft.com: All tests pass (including WebDriver New)
- arh.antoinevastel.com/bots/areyouheadless: Not detected
- bot-detector.rebrowser.net: 6/6 tests pass
- browserleaks.com: Clean fingerprint
