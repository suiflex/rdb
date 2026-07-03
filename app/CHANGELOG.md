# Changelog

## [0.4.0](https://github.com/suiflex/rdb/compare/v0.3.0...v0.4.0) (2026-07-03)


### Features

* **app:** dispatch write API through AnyDriver ([62e0726](https://github.com/suiflex/rdb/commit/62e072610395ff31f7139d4a9959cb78686b7b07))
* **app:** edit buffer model mapping buffered grid edits to WriteOps ([b60901b](https://github.com/suiflex/rdb/commit/b60901b1c2b404b7edb2dd03f4f878b84c539a1a))
* **app:** inline cell editing with buffered ⌘S commit ([365406b](https://github.com/suiflex/rdb/commit/365406ba97d6153802e0f1d64df56bff8c2e8188))
* **app:** live pagination in browse mode (page state, footer wiring, per-engine browse text) ([a9cdea3](https://github.com/suiflex/rdb/commit/a9cdea3f30ac53fccc3d8bd803ffd52e9b436bcb))
* **app:** modals, command palette, function view, empty state ([695d295](https://github.com/suiflex/rdb/commit/695d29509d2aa8ebe9b2e21eb1416c140dabd373))
* **app:** pagination + edit footer strip ([3b85166](https://github.com/suiflex/rdb/commit/3b851668b7875ffbd4a5b04827642f7313660ee8))
* **app:** pixel-matched SQL editor with incremental lexer + share bars ([25c2882](https://github.com/suiflex/rdb/commit/25c28826f3d80d5aeb7102483b71f7d23749b102))
* **app:** pixel-matched Tabula workspace (sidebar, grid, footer strip) ([1b13537](https://github.com/suiflex/rdb/commit/1b135373e2f9936d7dc01a3c504fd0a627d4f62d))
* **app:** sidebar filter + category counts, flat TablePlus-style tab bar ([d06a096](https://github.com/suiflex/rdb/commit/d06a09696be49262db7687fafbdddfe5e9ea7dca))
* **app:** TablePlus-style grid (gutter, zebra, hairlines, cell selection) ([3d4ece2](https://github.com/suiflex/rdb/commit/3d4ece26b804fd679a16f01ba399274c15bbff48))
* **app:** Tabula design system + pixel-matched Connections screen ([cf9bbff](https://github.com/suiflex/rdb/commit/cf9bbff4c4bff7073db4ae626ab7bc8fac61cda4))
* **app:** theme tokens for grid density + edit states ([ca4645f](https://github.com/suiflex/rdb/commit/ca4645fc374fe17a727fafc76da2123919e88cb6))
* **mongo:** write path via _id + skip pagination on find ([2b3a8af](https://github.com/suiflex/rdb/commit/2b3a8afddf916a27186b4f41ba707a31c1abf4ec))
* real-PostgreSQL end-to-end + driver type decoding + CI gate green ([ef08ba1](https://github.com/suiflex/rdb/commit/ef08ba1007a95773f989d7c1025d3c5030f9b1c3))

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
