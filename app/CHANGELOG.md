# Changelog

## [0.39.0](https://github.com/suiflex/rdb/compare/v0.38.0...v0.39.0) (2026-08-15)


### App Features

* **app:** restore query tabs at startup, not on first connect ([061d28c](https://github.com/suiflex/rdb/commit/061d28c7b1038197de9e1075943e5ea36e4d89b8))


### Bug Fixes

* **app:** keep query tabs inside RDB_STORE_DIR ([952ce08](https://github.com/suiflex/rdb/commit/952ce08b197f0b369a4136ac1cf2e7a9399bcc75))
* **app:** persist both panes of a split workspace ([59e722a](https://github.com/suiflex/rdb/commit/59e722ab9993ea695068da826c35a6206af7c5b6))
* **app:** stop Enter appending a newline in the details panel ([ca9e855](https://github.com/suiflex/rdb/commit/ca9e85520fbf87311cf280a43be6b823a7700fde))
* **app:** treat a Homebrew cask install as package-managed ([8d8f270](https://github.com/suiflex/rdb/commit/8d8f2705dff96ccc3d0b76da700e8809a0594faa))


### Performance Improvements

* **app:** build the displayed grid in one pass ([49e0681](https://github.com/suiflex/rdb/commit/49e0681deddb456f822aa6a871b5c4bf33b2632a))
* **app:** share result views instead of deep-copying them ([a6061f8](https://github.com/suiflex/rdb/commit/a6061f8bbb619e431b2d74c23eb57f97e4e9f83e))
