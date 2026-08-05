# Changelog

## [0.32.0](https://github.com/suiflex/rdb/compare/v0.31.0...v0.32.0) (2026-08-05)


### App Features

* **app:** bind the panel toggles and schema switch to keys ([a38ca36](https://github.com/suiflex/rdb/commit/a38ca3611dd82dfcc66c185f5fe5d0da1083ebc9))
* **app:** edit-on-click for single-table query results ([b059066](https://github.com/suiflex/rdb/commit/b059066ebd7fe24da569341c6ab2c4e11d2cfe07))
* **app:** editable query results, editor and completion fixes, header shortcuts ([49631fa](https://github.com/suiflex/rdb/commit/49631faf1c0adcf714ad6320f0be564fd745a92c))
* **app:** match completions without their underscores ([aa0aec1](https://github.com/suiflex/rdb/commit/aa0aec1d9e7eb79cc42b78051fad4cdcc878abb3))
* **app:** tooltip the header controls and name their shortcuts ([18c5e28](https://github.com/suiflex/rdb/commit/18c5e28a9100634536465a07f96bfe7722a41259))


### Bug Fixes

* **app:** apply the query-result PK lookup on the streaming path too ([14e4e95](https://github.com/suiflex/rdb/commit/14e4e950590e4ccfab08e40734eebdb11f209ce0))
* **app:** complete columns from a FROM that follows the cursor ([027ecb5](https://github.com/suiflex/rdb/commit/027ecb5d92ae18cd8c4f404dc270308c3da923dd))
* **app:** gate pane 1 grid edits on its own read-only flag ([af7a29c](https://github.com/suiflex/rdb/commit/af7a29cf1eea50df351bb5f75b5a4049d506f9f5))
* **app:** give find-match scroll a landing margin ([fbc7bba](https://github.com/suiflex/rdb/commit/fbc7bbab9b3dbc825051956540c01ce2f7bdbba3))
* **app:** ignore commented-out lines when detecting a single-table select ([755153a](https://github.com/suiflex/rdb/commit/755153a976a545be2b1bf8b3209315e8828f9c51))
* **app:** recognize schema-qualified tables for query-result edit ([61f5100](https://github.com/suiflex/rdb/commit/61f5100b18231f106473d1e18a6f31aef5cc959b))
* **app:** remove the run_stream PK-lookup race condition ([8a58b85](https://github.com/suiflex/rdb/commit/8a58b85a0d4ba15617bc0d63d30b1224a0e78f4b))
* **app:** run_sql PK lookup used the whole multi-statement buffer ([3a380e4](https://github.com/suiflex/rdb/commit/3a380e422c755a5ce5dd0600b097fb30af29e218))
* **app:** scroll editor to the cursor on every edit, not just find ([068f2c3](https://github.com/suiflex/rdb/commit/068f2c3fb97936b4e2a517a6588166e8bb8c251e))
* **app:** scroll query editor to active find match ([ec18e68](https://github.com/suiflex/rdb/commit/ec18e68a3fa9495999e864e6d8a4a6ec93f66b81))
* **app:** scroll the editor up as well as down ([b817a0f](https://github.com/suiflex/rdb/commit/b817a0f127d004965a23b5a93295b355f089c594))
* **app:** size the inline cell editor to its value ([1e8e9ab](https://github.com/suiflex/rdb/commit/1e8e9ab0537eb6f1ef406899ecc9e1bf66ace520))
* **app:** style the pending-edits Discard control as a real button ([2a7b545](https://github.com/suiflex/rdb/commit/2a7b545a6b99d5f564a8adc151b13390b5db05e7))
* **app:** use close icon for query find bar ([e7134f4](https://github.com/suiflex/rdb/commit/e7134f4cf4147694b3185096610eb3d777ffb7bf))
