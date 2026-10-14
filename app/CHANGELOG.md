# Changelog

## [0.49.0](https://github.com/suiflex/rdb/compare/v0.48.2...v0.49.0) (2026-10-14)


### App Features

* **app:** add column/table context actions, table import with row isolation, and UI polish ([1473680](https://github.com/suiflex/rdb/commit/14736800d32eac6ce66bc9bfb0d9599cbf68b741)), closes [#335](https://github.com/suiflex/rdb/issues/335)
* **app:** add Import JSON/CSV Wizard modal and resolve PR review findings ([0d62bd0](https://github.com/suiflex/rdb/commit/0d62bd04cdc6813b41f4d21aa8ae47e8397007fb))
* **app:** add tab focus and rebind choices to settings ([67701f9](https://github.com/suiflex/rdb/commit/67701f9ba7d96ed076ea3fd7e93757868482f7fa))
* **app:** dim a tab's connection dot while it is closed ([80c7961](https://github.com/suiflex/rdb/commit/80c7961e51a05906f5e6b62ebcedc99aa8a6be39))
* **app:** isolate query tab slot numbering and closed SQL scratchpads per connection ([cd4f83c](https://github.com/suiflex/rdb/commit/cd4f83c28405c26eb932f2e5373a68a9ddc473c6))
* **app:** land on each connection's last tab when switching ([9343e2c](https://github.com/suiflex/rdb/commit/9343e2ce7dc530ba0c7e461a9f56d8762f8217f6))
* **app:** let the sidebar or the tab decide where a tab runs ([a2702f8](https://github.com/suiflex/rdb/commit/a2702f846b1d4495d2450452e75e493b787b0ca6))
* **app:** regroup tabs when connection-scoped tabs flip ([dcecb6c](https://github.com/suiflex/rdb/commit/dcecb6c0a23e68848d4a57578998823ec0d0724c))
* **connstore:** add tab focus and tab rebind preferences ([1856cc2](https://github.com/suiflex/rdb/commit/1856cc2264815e18d2bf744cc815827dfffc1eda))


### Bug Fixes

* **app,driver-postgres:** handle E-string backslash escapes and transactional SQL import rollback ([6b55fbc](https://github.com/suiflex/rdb/commit/6b55fbc012a6b4ab0cf20660e32816e3d7c48ad8))
* **app,driver-postgres:** shield quoted/commented SQL in split_statements and resolve remaining PR review findings ([322260d](https://github.com/suiflex/rdb/commit/322260de989b860d89f344612b848050225808da))
* **app:** keep a deleted connection's tabs as unbound tabs ([277088d](https://github.com/suiflex/rdb/commit/277088d9913eab3f99f8b8d4bee0213163efc49c))
* **app:** keep each tab's cursor and selection across switches ([4665f2f](https://github.com/suiflex/rdb/commit/4665f2f5360a1b3f54a93de7f598433ff26148db))
* **app:** keep schema-switch fallback tab on the same connection ([480cda5](https://github.com/suiflex/rdb/commit/480cda5064a3ff1d404de8d0db815d5fda195039))
* **app:** keep tab drag-to-split on the left mouse button ([743ed85](https://github.com/suiflex/rdb/commit/743ed8555ea94a78e69deab29ffdf241f07c16a1))
* **app:** keep the topbar database and schema on the run target ([36a5979](https://github.com/suiflex/rdb/commit/36a5979b93760a503248075a0a23662e92c66843))
* **app:** keep the whole editor buffer when a tab runs ([5b95dae](https://github.com/suiflex/rdb/commit/5b95dae9d4134e6217db17f0632557e352373dc1))
* **app:** key in-flight query handles by tab instead of pane ([5ddacf9](https://github.com/suiflex/rdb/commit/5ddacf9813e4072eff4ef9f9a9501c29d7bb4d65))
* **app:** lay the production run confirmation out as a proper dialog ([0baf162](https://github.com/suiflex/rdb/commit/0baf1622647c8170941705bf48f91ed0afa22277))
* **app:** paint streamed rows only into the tab that runs them ([9d3221e](https://github.com/suiflex/rdb/commit/9d3221ecb41215d42dc545a2a378beea74c78e44))
* **app:** persist Oracle SQL imports ([ea685e0](https://github.com/suiflex/rdb/commit/ea685e02c307886f90552ccc34c47543397e85f4))
* **app:** refresh detail panel and tabs after editing a connection ([ce5233e](https://github.com/suiflex/rdb/commit/ce5233e72aaeb87d5cb73c7d889a27195b94755d))
* **app:** resolve every run through one tab-aware target ([f5c2704](https://github.com/suiflex/rdb/commit/f5c27044c75b727abc95cdd7f04f41e1cf533829))
* **app:** restore uppercase connection group labels ([9d311a4](https://github.com/suiflex/rdb/commit/9d311a454a6416f6b22776e391fe1c12f5d9b585))
* **app:** save open tabs before toggling connection-scoped tabs ([bb8f877](https://github.com/suiflex/rdb/commit/bb8f8778e5bdc7c1f5278be9f2fb8a50ab9adf6f))
* **app:** save the covered tab before opening a function view ([20fcf63](https://github.com/suiflex/rdb/commit/20fcf63ddc3a4736b1d932c401046945ac3f40ad))
* **app:** show engine badge for every engine in the connection modal ([193bd24](https://github.com/suiflex/rdb/commit/193bd242db3a3f9344615dde92f2b9acb6bc1c54))
* **app:** skip leading SQL comments when detecting explicit transaction control in imports ([26ce11d](https://github.com/suiflex/rdb/commit/26ce11df3b38328916c62b9fdf1db54e0ce47d7f))
* **deps:** bump quinn-proto to 0.11.19 for RUSTSEC-2026-0338..0342 ([bff22ee](https://github.com/suiflex/rdb/commit/bff22eeffe3ed3e2a53ac2b895ad1ee90d79537f))
* **drivers:** preserve row results for SQL batches ([29757a9](https://github.com/suiflex/rdb/commit/29757a9ae2c9ed5add803c249903e5f0b1527348))
* **import:** isolate engine-specific import execution ([8f7c84c](https://github.com/suiflex/rdb/commit/8f7c84cad15ee3e1717ffbe39ad599c766e76b32))
* **import:** preserve CSV and JSON values ([a1c837e](https://github.com/suiflex/rdb/commit/a1c837eaa1138b6df5579d0b6f5b9b479911260b))
* **tabs:** discard SQL text when closing tabs ([4dfcbcd](https://github.com/suiflex/rdb/commit/4dfcbcd9adb83c85f3005c980c3bba245cc6167b))
