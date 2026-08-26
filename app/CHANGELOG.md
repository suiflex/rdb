# Changelog

## [0.42.0](https://github.com/suiflex/rdb/compare/v0.41.0...v0.42.0) (2026-08-26)


### App Features

* **app:** choose the chart's label and value columns ([49ec211](https://github.com/suiflex/rdb/commit/49ec21197b627b4ed2122a5162c6087186c86c1e))
* **connstore:** resolve an engine from its badge key ([ea065e0](https://github.com/suiflex/rdb/commit/ea065e06fbdb8e5d762c12e5c2e400073103fc79))


### Bug Fixes

* **app:** clear the chart and details panel with the grid ([84af81d](https://github.com/suiflex/rdb/commit/84af81d9e27cecd63f3cf910d639b37289f5ed59))
* **app:** drop the completion popup on paste, undo and cursor moves ([3db5538](https://github.com/suiflex/rdb/commit/3db55383814fabcef5e259aea48dabd6b99c94e9))
* **app:** highlight and complete in the active tab's dialect ([631d437](https://github.com/suiflex/rdb/commit/631d4374f657bcb3672cc409487c78efc3681f14))
* **app:** keep tab action buttons alive while the pointer is on them ([d1d9257](https://github.com/suiflex/rdb/commit/d1d9257108d735e93c8921ed8da2b1903fd39127))
* **app:** keep the chart out of the details sidebar ([36aba68](https://github.com/suiflex/rdb/commit/36aba6819dcb955780ce9d8c041c3fb31b830add))
* **app:** keep the results view mode with the tab it belongs to ([c3c6f9c](https://github.com/suiflex/rdb/commit/c3c6f9c07232c28241018467917ea371b3cc0bb4))
* **app:** keep the right pane's tab when closing another one ([bbc6ce7](https://github.com/suiflex/rdb/commit/bbc6ce71541c0553fd5c4312a6f94a0c8eb100cb))
* **app:** read clause context from the statement, not the line ([93518aa](https://github.com/suiflex/rdb/commit/93518aa19d09e24b3bbe4f549e81715497279098))
* **app:** reset grid scroll to the top on a fresh result ([7799d48](https://github.com/suiflex/rdb/commit/7799d4826664cb84ccf6a4d0d59c4947ff80a7e9))
* **app:** resolve AS aliases and comma-joined FROM tables ([1bbd217](https://github.com/suiflex/rdb/commit/1bbd217ec6e3c14fddecc088be17902c55aeb105))
* **app:** scope dot completion to the qualified schema ([39925dd](https://github.com/suiflex/rdb/commit/39925ddf17fdf6facfd096ac7f3839ac90b678f2))
* **ci:** bump the version at $.package.version and seed version.txt ([e374958](https://github.com/suiflex/rdb/commit/e37495855d81c9c150bc33a577af56173c21f814))
* **ci:** root the release-please package at the repository ([f358c88](https://github.com/suiflex/rdb/commit/f358c88520e91b3be64f7184817c7b46b69462ac))
* **driver-postgres:** render enum values instead of NULL ([8cf228a](https://github.com/suiflex/rdb/commit/8cf228a2b8274077883e465f981d22037ae83d47)), closes [#253](https://github.com/suiflex/rdb/issues/253)
* **driver-postgres:** render time, timetz and interval values ([f4da7a7](https://github.com/suiflex/rdb/commit/f4da7a7da18a3748332e705472fca54171b92035)), closes [#253](https://github.com/suiflex/rdb/issues/253)
* **driver-postgres:** show enum type name in the schema tree ([01cbf7a](https://github.com/suiflex/rdb/commit/01cbf7ab8d36bee0f88a30cf0078d73611111afa)), closes [#253](https://github.com/suiflex/rdb/issues/253)
* **tunnel:** bump russh to 0.61.1 for the pre-auth DoS advisories ([07fde60](https://github.com/suiflex/rdb/commit/07fde60f462e6b4cd11fc48aaf2ed5eea9cb566f))
* **tunnel:** connect to the SSH agent on Windows too ([41a4631](https://github.com/suiflex/rdb/commit/41a4631c38c1fb321b6cc84299989b2a3357deea))
