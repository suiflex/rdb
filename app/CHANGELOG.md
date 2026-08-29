# Changelog

## [0.44.0](https://github.com/suiflex/rdb/compare/v0.43.0...v0.44.0) (2026-08-29)


### App Features

* **app:** browse Mongo collections with mongosh syntax ([3b6f39d](https://github.com/suiflex/rdb/commit/3b6f39d8021e0203e658c6d810212e396e4bb165))
* **app:** clear an applied filter from the filter box ([435bc1b](https://github.com/suiflex/rdb/commit/435bc1b21e4ca783a3be27641401d661a9437124))
* **app:** dispatch Oracle connections to the driver ([56f5446](https://github.com/suiflex/rdb/commit/56f5446830632cfa50f0ea406bdb2f81f9d7dfff))
* **app:** teach the app Oracle's SQL dialect ([d3f5008](https://github.com/suiflex/rdb/commit/d3f50086f30ec24593204d0603be58d646c50580))
* **app:** wire Oracle into the connection UI ([09b825d](https://github.com/suiflex/rdb/commit/09b825d14918bb427b694c0969bbbc3d8267fc4c))
* **connstore:** add the Oracle engine and its oracle:// scheme ([2287704](https://github.com/suiflex/rdb/commit/22877043433d7e00fe85006123556395a453d6b9))
* **driver-oracle:** add Oracle driver crate ([ba476ce](https://github.com/suiflex/rdb/commit/ba476ce5881307d913aec16f4d758d03c8aa885c))


### Bug Fixes

* **app:** browse collections whose name contains a dot ([5723dba](https://github.com/suiflex/rdb/commit/5723dba53d47cf5b377bb237bf2cbd4358ef5165))
* **app:** center the fold arrow in the editor gutter ([dc64d60](https://github.com/suiflex/rdb/commit/dc64d608b3a6c10906438eece4cb6156f4bd908d))
* **app:** give the fold arrow a glyph that survives 10px ([9f1daf7](https://github.com/suiflex/rdb/commit/9f1daf77d5a8873c7c60bfab7c704c2fead107e2))
* **app:** hide the filter placeholder while the field is focused ([d94012d](https://github.com/suiflex/rdb/commit/d94012def3c2f1b2df7c8a699a7a0a729bcc018d))
* **app:** highlight the rest of the mongosh vocabulary ([90b0e09](https://github.com/suiflex/rdb/commit/90b0e098e28c77c9cb0b310cd1679599a08b5909))
* **app:** keep result tabs on a browse tab so Run New works ([74f3b57](https://github.com/suiflex/rdb/commit/74f3b57b7449bac73b7951c9d2a9543fc92db339))
* **app:** render the ClickHouse badge icon ([a38ba8b](https://github.com/suiflex/rdb/commit/a38ba8b74feea4fc44c8092be5afa0074d25f858))
* **app:** repaint the Mongo filter box on tab switch ([788e6db](https://github.com/suiflex/rdb/commit/788e6db3df5f46f4ace663371c73e1208854a927))
* **app:** respect single-quoted strings when scanning Mongo calls ([d949286](https://github.com/suiflex/rdb/commit/d949286d7d84f63d0d76c76972d72edbe1f8d051))
* **app:** stop swallowing the Mongo browse filter ([c8897af](https://github.com/suiflex/rdb/commit/c8897af0a9d952b40b26dbe2aad0599e14da6fcd))
