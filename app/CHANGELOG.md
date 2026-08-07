# Changelog

## [0.33.0](https://github.com/suiflex/rdb/compare/v0.32.1...v0.33.0) (2026-08-07)


### App Features

* **app:** add drag/shift-click row-range selection to results grid ([90206f5](https://github.com/suiflex/rdb/commit/90206f5cd63177a6b1aebf8e93e1cb30e851e322))
* **app:** enrich rows-affected toast with verb and latency ([d81e420](https://github.com/suiflex/rdb/commit/d81e4209f4cffc705c4d3ade89f8b33a1c6c42c2))
* **app:** scope range copy to the anchor column ([43c68ca](https://github.com/suiflex/rdb/commit/43c68ca88b0990ac747e549e707ae2e93ce75e96))
* **app:** turn the palette into real global search ([b18b6d8](https://github.com/suiflex/rdb/commit/b18b6d8cedfaa9e6d0b19a23fa15066911a0ac32))
* **app:** wire range-select copy for the results grid ([a23448b](https://github.com/suiflex/rdb/commit/a23448b6053233f95f8b6a1496c047408ff05edf))


### Bug Fixes

* **app:** detect verb in rows-affected toast past leading comments ([59d8fbb](https://github.com/suiflex/rdb/commit/59d8fbbb1d87e922537029f9e9e6a1ec70923e54))
* **app:** let the results grid claim focus so Cmd+C copies the range ([1f35229](https://github.com/suiflex/rdb/commit/1f35229c56b73fe6decc360e5560c10c8b0c3d50))
* **app:** stop current-statement fallback from running an unrelated statement ([fcb931d](https://github.com/suiflex/rdb/commit/fcb931d7c5fda76142c63bcd2b2cc2180c1486c5))
* **app:** trim leading comments from the statement sent to Run ([a355490](https://github.com/suiflex/rdb/commit/a3554907ec601b4edc64b6ef82e31b9c999c7fa1))
* **app:** wire Cmd+C to the range-aware results copy ([936a457](https://github.com/suiflex/rdb/commit/936a4574019c55b69a9a8decb9ca29c650b895b5))
