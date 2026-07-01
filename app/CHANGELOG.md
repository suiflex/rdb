# Changelog

## [0.3.0](https://github.com/suiflex/rdb/compare/v0.2.0...v0.3.0) (2026-07-01)


### Features

* **mongo:** bound collection browse with a row limit ([e1de976](https://github.com/suiflex/rdb/commit/e1de976cc32fd4d7ea7143a3071876056d9a512d))


### Bug Fixes

* **mongo:** correct sidebar tree and data preview ([14a3e5e](https://github.com/suiflex/rdb/commit/14a3e5e15e30800d9c4b97fa6e04e56dc3655343))
* **ui:** show loading/empty placeholder for mongo databases ([d390b24](https://github.com/suiflex/rdb/commit/d390b248eb176c4c1de3faf535d8e98f5ca8978d))

## [0.2.0](https://github.com/suiflex/rdb/compare/v0.1.0...v0.2.0) (2026-06-22)


### Features

* **app:** AnyDriver dispatch, model conversion, accent parse, tokio&lt;-&gt;slint bridge ([9b25ea2](https://github.com/suiflex/rdb/commit/9b25ea28a8ae07920c612eff56569a6c6b5b9b1d))
* **app:** connection CRUD wiring (single ConnStore, add/edit/delete + keychain) ([f957544](https://github.com/suiflex/rdb/commit/f957544a56420b82ae119e37122a0b55a5b31c14))
* **app:** connection form modal component (visual shell) ([2fdf009](https://github.com/suiflex/rdb/commit/2fdf00935af515e30cc7367f6942af45f4a2d099))
* **app:** engine-aware query parsing (SQL/Redis/Mongo from one editor) ([bf3df7e](https://github.com/suiflex/rdb/commit/bf3df7e39e2004ed34c716ea3341b7b375b05a78))
* **app:** route run-query through engine-aware parse_query ([41086c3](https://github.com/suiflex/rdb/commit/41086c322f35e6bee5201be47381d3f52fb21934))
* **app:** Theme tokens, 3-pane shell, sidebar, workarea, palette (.slint) ([b4fe2f7](https://github.com/suiflex/rdb/commit/b4fe2f7c10ba7f473e6b0031051b423fddaf7ba3))
* **app:** wire mysql/redis/mongo into AnyDriver dispatch ([7c64b78](https://github.com/suiflex/rdb/commit/7c64b78d78ba102b54e71c3b1c4fd039ed9a5b82))
* flatten mongo documents into tabular grid ([cd0f77b](https://github.com/suiflex/rdb/commit/cd0f77bc8801da1543972af6b376bb8c7ea516b8))
* lazy-load mongo collections, capped per database ([a39bd67](https://github.com/suiflex/rdb/commit/a39bd67afb32c0a51c29abf90b85da9e271fe493))
* let mongo ops target a specific database ([8e6b51d](https://github.com/suiflex/rdb/commit/8e6b51d3d6926bb6bce101faf02de342a4d6460e))
* make post-connect view engine-aware for mongo/redis ([734a691](https://github.com/suiflex/rdb/commit/734a6917b2755cbcd13e049d343bc1f1df3142b6))
* render mongo sidebar as database to collection tree ([f5fc812](https://github.com/suiflex/rdb/commit/f5fc8125f53d8d2060403a32a19da8ec20f5be0b))


### Bug Fixes

* **app:** reject empty/invalid port in connection form instead of saving 0 ([8fbb5bd](https://github.com/suiflex/rdb/commit/8fbb5bdfebb495bb9794c5890c9cf83869200f37))
* **app:** use PostgreSQL label consistently in conn-form combobox + fmt ([45330e5](https://github.com/suiflex/rdb/commit/45330e5802dc42b8e89318cecea6da7379ccdec5))
* bound test-connection and recover form state on cancel ([d764576](https://github.com/suiflex/rdb/commit/d76457613d63712b769cea4fa622c8eeaf78ed30))
* connection dialog + MongoDB database/collection browser ([67d48be](https://github.com/suiflex/rdb/commit/67d48be64155d8ba1fadb3dee96435bb86443135))
* keep connection form buttons inside the card, hide unused fields ([a2c1b9e](https://github.com/suiflex/rdb/commit/a2c1b9e733844e6eea6181af46f5726d44eb961a))
* pin raw-JSON document view to top-left ([89a184a](https://github.com/suiflex/rdb/commit/89a184a666d65ec8608c37d8b7d236e05010750f))
* size connection card to its content, not the overlay ([59eab83](https://github.com/suiflex/rdb/commit/59eab836cc942b538b2fe129f64f7dcd540b2dfc))
* stop sidebar tree jitter and tidy collection rows ([15d115e](https://github.com/suiflex/rdb/commit/15d115e7c91ea40328f9f35b7696ecf049133346))
