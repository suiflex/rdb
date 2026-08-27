# Changelog

## [0.43.0](https://github.com/suiflex/rdb/compare/v0.42.0...v0.43.0) (2026-08-27)


### App Features

* **app:** select a block of cells across columns ([718cd4a](https://github.com/suiflex/rdb/commit/718cd4a3b308cd44aa06749cd827779ddbc19688))


### Bug Fixes

* **app:** copy only the selected cells ([997632a](https://github.com/suiflex/rdb/commit/997632a139ab26e4942a19d8b0174fa189c5256b))
* **app:** keep the editor caret on the pixel grid ([4d506c3](https://github.com/suiflex/rdb/commit/4d506c36ec9823887b0932d43fc4dbe9c8b729fd))
* **app:** let Enter run the Mongo browse filter ([8491130](https://github.com/suiflex/rdb/commit/8491130ed45fd82dbf95d20ef710396ac4646698))
* **app:** open a query in a new tab instead of over the current one ([bed79cd](https://github.com/suiflex/rdb/commit/bed79cd581bc1b777f5be5c0d513a4dd9cbd4007))
* **app:** release the tab lock before persisting on close ([5efd731](https://github.com/suiflex/rdb/commit/5efd731580e237017665ab834409982c4be3469b))
* **app:** stop the results grid panning on a selection drag ([923fb26](https://github.com/suiflex/rdb/commit/923fb267a8b85dd6d63f4938a01c7695de3d4cba))
* **driver-mongo:** encode credentials the user typed into the URI ([9570e5a](https://github.com/suiflex/rdb/commit/9570e5af0e99df44ce944b87f3faaa5d1fd31676))
* **driver-mongo:** keep Decimal128 exact instead of parsing it ([db10bb6](https://github.com/suiflex/rdb/commit/db10bb6f851a0d60f7a4351be6dd8e9f4ea27b7d))
* **driver-mongo:** stop reporting a write that matched nothing as applied ([6fd9486](https://github.com/suiflex/rdb/commit/6fd9486254db90151a7958c25d15cd98f356a5c6))
