# Changelog

## [0.48.0](https://github.com/suiflex/rdb/compare/v0.47.1...v0.48.0) (2026-09-25)


### App Features

* **driver-oracle:** highlight the failing token from the server offset ([62c8fd6](https://github.com/suiflex/rdb/commit/62c8fd6979e901e71b0b05e6a041923018d4c1f5))


### Bug Fixes

* **app:** connect a tab's connection instead of half-switching to it ([3dc3c33](https://github.com/suiflex/rdb/commit/3dc3c33a5d11ce1b5222081648f96b98fef3c1cf))
* **app:** drop a stale connect result instead of repainting over it ([9bece1f](https://github.com/suiflex/rdb/commit/9bece1f475e6906a54cad943a680359158ddb04d))
* **app:** focus a tab of the connection being connected ([5a0541a](https://github.com/suiflex/rdb/commit/5a0541acfb1230c453cb6d3a34d577244f371db5))
* **app:** follow the focused tab's connection as the active context ([7381bea](https://github.com/suiflex/rdb/commit/7381bea586150f440226da7fc23aaf7ec3b414a2))
* **app:** reactivate the survivor when one of two connections drops ([d6a157b](https://github.com/suiflex/rdb/commit/d6a157bf4f7633829f9e4a09f514650697f27a0f))
* **app:** resolve right-pane actions against the right pane's tab ([b31acf1](https://github.com/suiflex/rdb/commit/b31acf15da0cba0900ac1f6d5842db3d7d65ef39))
* **app:** resolve the sidebar tree's driver through the pool ([6ce4c93](https://github.com/suiflex/rdb/commit/6ce4c93fa43f1f6aba07aedb95ef2fc5c894efc4))
* **app:** tighten the connection switch against its own edge cases ([a790167](https://github.com/suiflex/rdb/commit/a7901678254a831831dc8fe21064a5286a3d95d6))


### Performance Improvements

* **app:** keep the outgoing workspace state when switching from the rail ([0fea21e](https://github.com/suiflex/rdb/commit/0fea21e0ddc65c378c5eb76fd11af0cd7392bb95))
* **app:** swap cached workspace state instead of reconnecting ([2cb0329](https://github.com/suiflex/rdb/commit/2cb032916c1189618a515c02ad02c48b34f85345))
