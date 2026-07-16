# Changelog

## [0.17.0](https://github.com/suiflex/rdb/compare/v0.16.0...v0.17.0) (2026-07-16)


### Features

* **header:** show theme toggle on connections screen too ([d952ac0](https://github.com/suiflex/rdb/commit/d952ac0ec17c5f99955169de99a57dc9912b56be))
* **picker:** add engine brand glyph assets + icon mappings ([4b8506c](https://github.com/suiflex/rdb/commit/4b8506cfb3d29ad92e9676f7fbbf55c72a839a01))
* **picker:** engine-tinted connection cards, brand logos, header theme toggle ([5cd1a9d](https://github.com/suiflex/rdb/commit/5cd1a9dcc447b552baf0c6183900a59f29da18e3))
* **picker:** show engine brand logo on connection badge ([e3c2269](https://github.com/suiflex/rdb/commit/e3c22690e25cd5a6a7ce8d3e974ab61aaf8b13a4))
* **picker:** show RDB logo on empty connection panel ([d0ad469](https://github.com/suiflex/rdb/commit/d0ad4690aeff0d1a9c9b8b331a2a48c7936d430d))
* **picker:** tint connection row card per engine color ([c8c523c](https://github.com/suiflex/rdb/commit/c8c523c52a2191350a8d9b9bb7af064d726e0784))
* **ui:** add Tooltip global + shared overlay layer ([ccbe616](https://github.com/suiflex/rdb/commit/ccbe61675d9914f31ac8ec28376461d360e842bd))

## [0.16.0](https://github.com/suiflex/rdb/compare/v0.15.0...v0.16.0) (2026-07-16)


### Features

* **grid:** stream "No limit" results progressively with cancel ([c47ff28](https://github.com/suiflex/rdb/commit/c47ff28af639965567f9b6d9e7e082ac4cf3d680))


### Bug Fixes

* **grid:** cap manual SELECT rows and virtualize the result grid ([43f06e0](https://github.com/suiflex/rdb/commit/43f06e0778f8aa426d8200d3481e841606c7977d))
* **grid:** fix SELECT * freeze — row cap, virtualized grid, streaming No-limit ([9fce5b5](https://github.com/suiflex/rdb/commit/9fce5b55f1cc5f73247550d4bf947c91071c9e60))

## [0.15.0](https://github.com/suiflex/rdb/compare/v0.14.0...v0.15.0) (2026-07-15)


### Features

* **app:** native Save-As dialog for CSV export ([1b5878e](https://github.com/suiflex/rdb/commit/1b5878e488074e1c5d501f23333ad9a2160f415d))


### Bug Fixes

* **ui:** panel toggles eaten by tooltip; native Export Save-As ([3363eb5](https://github.com/suiflex/rdb/commit/3363eb5e9d64102f555d40b8cba0c6824c42a507))
* **ui:** remove redundant footer Export buttons ([4c567a4](https://github.com/suiflex/rdb/commit/4c567a4242b858fcb5a21cfe3a6287053be6e371))
* **ui:** stop tooltips from swallowing button clicks ([6c4c819](https://github.com/suiflex/rdb/commit/6c4c819b05b0bcf01feeee87e25d0bce06b908e4))

## [0.14.0](https://github.com/suiflex/rdb/compare/v0.13.0...v0.14.0) (2026-07-15)


### Features

* **app:** database switch dropdown ([dddc676](https://github.com/suiflex/rdb/commit/dddc6769e7fa7c602892b80ee1c7bbf79c8f6824))
* **app:** gate autocomplete to typed prefix and flip popup when clipped ([4eeddc9](https://github.com/suiflex/rdb/commit/4eeddc9ac6a71404c16dded5db680e9e49a38acc))
* **app:** persist query tabs and text across restarts ([23ac688](https://github.com/suiflex/rdb/commit/23ac688f4f7fc8a18a56dac802d9cedffee70f49))
* autocomplete fixes, header/sidebar UX, Details panel, npm dist ([de692d1](https://github.com/suiflex/rdb/commit/de692d111f63cad17d0527ec3e8a0fe25b786432))
* query UX fixes — db errors, join columns, db switch, tab persistence ([395001a](https://github.com/suiflex/rdb/commit/395001a02a07977dc23782346940aeb8eed1302b))
* **ui:** add About RDBS dialog with version ([dbb2e99](https://github.com/suiflex/rdb/commit/dbb2e990bf6783f5e4eec3cf15c3d0a318b6cefd))
* **ui:** add collapse and reveal buttons for the sidebar ([aa68a92](https://github.com/suiflex/rdb/commit/aa68a925bb6d383428455cc23e29c78bf4c19281))
* **ui:** add native menu bar (File/Edit/View/Help) ([54e4317](https://github.com/suiflex/rdb/commit/54e43171bd964f9535dd6ab397e2ca478e590c68))
* **ui:** add New Query button to the empty workspace ([3674f58](https://github.com/suiflex/rdb/commit/3674f586b5743afcae5947baef769cb1f3393309))
* **ui:** add optional tooltip to IconButton and SecondaryButton ([2168220](https://github.com/suiflex/rdb/commit/2168220ed7f0e729a5fcba68d36189fffeece8fa))
* **ui:** dedicated collapse/expand icons for the sidebar ([6e152ee](https://github.com/suiflex/rdb/commit/6e152ee78ebcd40dd43cc7c48a856f00aff9a441))
* **ui:** improve data grid editing and details ([48e8968](https://github.com/suiflex/rdb/commit/48e896867636dd38ab1e9812987d1f34e49b9b49))
* **ui:** improve data grid editing and native releases ([7e5d0c9](https://github.com/suiflex/rdb/commit/7e5d0c9995f651e4afc9fe57eb70722fae2db6a0))
* **ui:** tab/console/menu polish + audit & dependabot CI ([b731c53](https://github.com/suiflex/rdb/commit/b731c53a103e929fe259666061a9ac97165a72eb))
* **ui:** tooltips on header buttons with shortcut hint ([db103a7](https://github.com/suiflex/rdb/commit/db103a7846fffe37f9c482b9d1eda883a4e95fe0))


### Bug Fixes

* **app:** disambiguate duplicate result column names ([1642d0f](https://github.com/suiflex/rdb/commit/1642d0f378ed5c56071126a346813ba1fae506c5))
* **app:** ignore semicolons in comments when running statement under cursor ([a7e53a7](https://github.com/suiflex/rdb/commit/a7e53a737cfa0a2ba93672b14f4e278068c53130))
* **app:** offer all schema names in autocomplete immediately ([99e7cff](https://github.com/suiflex/rdb/commit/99e7cffb1286f0999d6e10952a14a10f390df530))
* **app:** resolve schema-qualified table aliases in autocomplete ([d7bf873](https://github.com/suiflex/rdb/commit/d7bf873662ad15d53c341f493e19cf6c6c710ec5))
* **app:** resolve table aliases across lines in autocomplete ([c5f6f90](https://github.com/suiflex/rdb/commit/c5f6f904e0ca6982b54828cb34af6fe70c9a5adf))
* **app:** send SQL to the engine verbatim, don't strip comment lines ([eea1e59](https://github.com/suiflex/rdb/commit/eea1e59e61a25cc5879dde493273eb4d701a3e35))
* **app:** stop force-opening query console on each run ([99662ce](https://github.com/suiflex/rdb/commit/99662ce6cb5f7fa652a07dc96f128fd6e7959ccd))
* **ui:** auto-open SQL console on run and swap toggle icon ([048ba76](https://github.com/suiflex/rdb/commit/048ba766cde352070d4eee167f9bd40a88cfd75b))
* **ui:** draw two rows in the rows icon, not three ([bb95d97](https://github.com/suiflex/rdb/commit/bb95d97365260b4ea50b37c145dbcf014d49809e))
* **ui:** log only user queries in the SQL console ([1ea6fd3](https://github.com/suiflex/rdb/commit/1ea6fd38c09766e570b8b874ea928263072c510c))

## [0.13.0](https://github.com/suiflex/rdb/compare/v0.12.0...v0.13.0) (2026-07-14)


### Features

* **ui:** add TablePlus-style workspace tabs ([55987a1](https://github.com/suiflex/rdb/commit/55987a11305b4e0b2ba419b848e45e779954911b))
* **ui:** add TablePlus-style workspace tabs ([7c98c15](https://github.com/suiflex/rdb/commit/7c98c15e2763991200876c1c99128bf9badfb582))

## [0.12.0](https://github.com/suiflex/rdb/compare/v0.11.0...v0.12.0) (2026-07-13)


### Features

* **app:** add a collapsible JSON tree model for Mongo documents ([61b3183](https://github.com/suiflex/rdb/commit/61b318332cdc627f1ce3a1f8655acd9eb8568d81))
* **app:** filter the Mongo sidebar to the selected database ([3a85091](https://github.com/suiflex/rdb/commit/3a85091e9a24c66ad4de91b0c25cbe82ca39e72f))
* **app:** parse mongosh line syntax in the Mongo editor ([9c83c92](https://github.com/suiflex/rdb/commit/9c83c925ce2ceed5636937da6ef8340c4298ea0e))
* **app:** suggest clause keywords and schema tables in autocomplete ([cfa93d6](https://github.com/suiflex/rdb/commit/cfa93d6298e79aa8551b71df7d13fb6f346190e6))
* **completion:** schema-aware cross-schema SQL autocomplete ([a7f447e](https://github.com/suiflex/rdb/commit/a7f447e978735963d42563cf679ac8d20d32b9f0))
* **core:** add sort field to MongoOp ([fe64f0f](https://github.com/suiflex/rdb/commit/fe64f0f60f2a4b5d079f6a89685a5dd8472d5389))
* mongosh query UX for Mongo + autocomplete/schema fixes ([16d945f](https://github.com/suiflex/rdb/commit/16d945fac5562ceca65a9d3fa0e5701703b1e7cb))
* **ui:** add keyword and field autocomplete icons ([a43d9cd](https://github.com/suiflex/rdb/commit/a43d9cd67510e9e97dd4eee4a471629cae7613b8))
* **ui:** clickable breadcrumbs, centered search, panel toggles ([d1fd6f6](https://github.com/suiflex/rdb/commit/d1fd6f635e91879e4e4101d3c5494ec1baaacdb5))
* **ui:** Compass-style filter bar for Mongo browse ([6555d5d](https://github.com/suiflex/rdb/commit/6555d5df59528545787ce6b350f82a2e603d36f8))
* **ui:** grid/tree toggle for Mongo documents ([44065cc](https://github.com/suiflex/rdb/commit/44065cc151cfa84d680f8f3fb91dc9557f1037b6))
* workspace header UX + schema-aware autocomplete ([0e03878](https://github.com/suiflex/rdb/commit/0e038780aa746b628b09fecff32b0c1b934b5f9d))


### Bug Fixes

* **app:** accept relaxed mongosh JSON and use() in the Mongo editor ([16456ae](https://github.com/suiflex/rdb/commit/16456ae519f245bd2960c9015dc86de2b02173c8))
* **app:** default Mongo browse to 20 docs per page ([716493d](https://github.com/suiflex/rdb/commit/716493d30a1d8dbce603301dcc47566cbf2582a0))
* **app:** isolate the query buffer per tab ([9b44d9f](https://github.com/suiflex/rdb/commit/9b44d9f60550cd5a815a420ea7c475bc46025071))
* **app:** run Mongo editor queries against the selected database ([ea87014](https://github.com/suiflex/rdb/commit/ea870141e2768bcffd5c7dcf734eb6b7823ffc42))
* **mongo:** connect fixes + document browsing UX ([42ef9b5](https://github.com/suiflex/rdb/commit/42ef9b55c92bed4d172d01f6d14273668ed5c371))
* **ui:** keep the query editor available while browsing ([8bf79db](https://github.com/suiflex/rdb/commit/8bf79db4b56f97872dab83bdd77b440956046842))
* **ui:** make the app icon background transparent ([0a1e2cd](https://github.com/suiflex/rdb/commit/0a1e2cd68454496882275401e524e2342c206367))
* **ui:** make the Mongo JSON tree usable ([4e832a0](https://github.com/suiflex/rdb/commit/4e832a08dd0696a80ca3ebc03026204b48e6515b))
* **ui:** move the Grid/Tree toggle into the browse toolbar ([447e034](https://github.com/suiflex/rdb/commit/447e0349c28adfb66eb3856b2bfb194917b1834e))
* **ui:** render autocomplete suggestion icons as SVGs ([9f69f13](https://github.com/suiflex/rdb/commit/9f69f1350f3821998af3b3e20fa787e2bb87363f))
* **ui:** render inline autocomplete icons as SVGs ([3f6eedd](https://github.com/suiflex/rdb/commit/3f6eedd1810739ea4e9005c7d5018e59a8871306))
* **ui:** render the browse refresh button as an SVG icon ([22c000e](https://github.com/suiflex/rdb/commit/22c000efe52ba93b15dea11a6cb26d29ab3e0766))
* **ui:** show button tooltips via a popup instead of a clipped rect ([318979c](https://github.com/suiflex/rdb/commit/318979cf53fb476806baeb69b0fa4be6f09d4689))


### Performance Improvements

* **app:** cap Mongo document tree scalar previews ([65270e7](https://github.com/suiflex/rdb/commit/65270e7effb20612101f08fd494009d9763e05c6))

## [0.11.0](https://github.com/suiflex/rdb/compare/v0.10.0...v0.11.0) (2026-07-12)


### Features

* **app:** add Options/URI field to the connection form ([e5d9b34](https://github.com/suiflex/rdb/commit/e5d9b34d7091d532ae3648982302d0d68f967093))
* **brand:** align RDB logo + README with suiflex org ([a88d56d](https://github.com/suiflex/rdb/commit/a88d56dfe84bc110976eec9e662ab6f0f98d5dc4))


### Bug Fixes

* **editor:** full SQL autocomplete with clean identifier insert ([e054a84](https://github.com/suiflex/rdb/commit/e054a84568c10ffbb5988d5822bf7bfb57d38637))
* mongo connect, SQL autocomplete, and editor toolbar UX ([b984bf0](https://github.com/suiflex/rdb/commit/b984bf0ebfd635f52d54229dd2b94f0a1b877298))
* **ui:** label the disconnect button with a hover tooltip ([95e73f3](https://github.com/suiflex/rdb/commit/95e73f344db599ed5bf7663b90b4013346f7497d))
* **ui:** right-align the Limit stepper ([2f6a693](https://github.com/suiflex/rdb/commit/2f6a693d4b8f98f62734358d086961312d84b140))

## [0.10.0](https://github.com/suiflex/rdb/compare/v0.9.0...v0.10.0) (2026-07-11)


### Features

* add app settings store (theme, UI state, prefs) ([be03685](https://github.com/suiflex/rdb/commit/be03685c06981a8253adbe0bbeac076d50d2cdc4))
* add settings modal (theme + update-check) ([54ea34b](https://github.com/suiflex/rdb/commit/54ea34bd959236d0796fdad0d8fe545879ca37c0))
* **app:** check for updates and show install-aware reminder ([4b4fd8d](https://github.com/suiflex/rdb/commit/4b4fd8dd5699b93b8b95b5b7a5bd7f5babb8dcf6))
* **app:** collapse Functions category by default ([1d78b19](https://github.com/suiflex/rdb/commit/1d78b198dd683beba373a0e660859ca8bd94af26))
* **app:** expand table columns on single-click, open on double-click ([0a011f7](https://github.com/suiflex/rdb/commit/0a011f76416784b63f78da9eab5098f07984904c))
* **app:** persist and restore theme and collapsed groups ([0fe20e9](https://github.com/suiflex/rdb/commit/0fe20e9197a634101315e764a7e62523e75e4c85))
* **app:** wire settings modal update-check toggle ([188a920](https://github.com/suiflex/rdb/commit/188a92006dd60ec37b8d77427372d3287c2b4b86))
* check for updates with install-aware reminder ([a838e2e](https://github.com/suiflex/rdb/commit/a838e2e4e7ce0c3f69161e86ce90b36b6bed160e))
* **ui:** add pencil icon asset and register in AppIcon ([7b4525f](https://github.com/suiflex/rdb/commit/7b4525f1edef291d473b8ef88fec97e68da34f81))
* **ui:** add settings modal with theme and update-check toggles ([68bc4aa](https://github.com/suiflex/rdb/commit/68bc4aae46ae02972f524bbc8c640d139d4ac977))
* **ui:** add star icon asset and register in AppIcon ([f7e5dd3](https://github.com/suiflex/rdb/commit/f7e5dd39b8fc10a8e07a1b05f61bcecfff5442b1))
* **ui:** add update-reminder banner ([b22ff60](https://github.com/suiflex/rdb/commit/b22ff6091b374b1e63199e95f8980da7a0cf177d))
* **ui:** caret state indicator and dedicated saved/recent query icons ([6189b2a](https://github.com/suiflex/rdb/commit/6189b2af34df04f592c60c389e8652623a136480))
* **ui:** distinct pencil icon for connection settings button ([babe3ab](https://github.com/suiflex/rdb/commit/babe3abe1ce7452668d9c7b65929d4ea01b4d2f1))
* **ui:** focus sidebar filter with Cmd-P ([cadb112](https://github.com/suiflex/rdb/commit/cadb112e4399b06a10bd8ed07ef168c6fa938d31))
* **ui:** sidebar column expand, dedicated icons, palette & toolbar fixes ([82b25e3](https://github.com/suiflex/rdb/commit/82b25e3db9d9bbc01fa94f9562199e64d7df030e))


### Bug Fixes

* **app:** collapse Functions on initial connect and schema switch ([25e73fa](https://github.com/suiflex/rdb/commit/25e73faca8cdb8b5a9a32a6c2a416df3937826ad))
* **ui:** cap command palette height so footer stays visible ([4557cdf](https://github.com/suiflex/rdb/commit/4557cdfa407021e6a9a7ef48e4b3774e78e14ce3))
* **ui:** even spacing across toolbar action icons ([627e063](https://github.com/suiflex/rdb/commit/627e063ef6afc327452c682f26ed6126a773258c))
* **ui:** keep Limit label next to its stepper buttons ([39624dd](https://github.com/suiflex/rdb/commit/39624ddcc3ad7192a1222e8d9201e3b7410860ba))

## [0.9.0](https://github.com/suiflex/rdb/compare/v0.8.0...v0.9.0) (2026-07-10)


### Features

* **app:** add disconnect button to top bar ([b67dcc4](https://github.com/suiflex/rdb/commit/b67dcc471fda55b93ee98ceb8546bfaa5ec9ecfc))
* **app:** add editor comment toggle ([8ea6144](https://github.com/suiflex/rdb/commit/8ea6144b0e7b959fe43579c7a7b3376e53a2dbaa))
* **app:** cancel an in-flight connection attempt ([21233c4](https://github.com/suiflex/rdb/commit/21233c456f35c89caeff7283dcf7a678c50639ed))
* **app:** collapse the SQL editor pane ([c0f1dc9](https://github.com/suiflex/rdb/commit/c0f1dc96f90684a70c989ed96af359c3d7fc0c9c))
* **app:** compute per-statement line spans for editor folding ([0f05232](https://github.com/suiflex/rdb/commit/0f05232b2aa4fc54b599242b39e9fd9c12f6378e))
* **app:** per-statement code folding in the SQL editor ([8dceecb](https://github.com/suiflex/rdb/commit/8dceecbc1cd37872186cde6956a8ddfa632ac302))
* **app:** persist recent query history to disk ([997371f](https://github.com/suiflex/rdb/commit/997371f36701f65a16559871901c778d0d8fe014))
* **app:** refetch table sidebar when switching schema ([93dd533](https://github.com/suiflex/rdb/commit/93dd533aff6c1de5c9f0082ecf7c0d0081f41371))
* **app:** rename query tabs via a modal ([5d0b72c](https://github.com/suiflex/rdb/commit/5d0b72c2174860100eaf4ad56e7ab7c7ef77ed34))
* **app:** schema switch refetch, connect cancel, tab rename, editor folding ([b36306c](https://github.com/suiflex/rdb/commit/b36306cc30a6abe9817d2d6231ff2ed69cfe092a))
* **app:** strip line comments per engine in query_parse ([4bd6f3d](https://github.com/suiflex/rdb/commit/4bd6f3de899eb8d1a63956117b1c2910adc80fd8))
* **app:** wire Cmd+/ comment toggle in editor key handler ([35b091e](https://github.com/suiflex/rdb/commit/35b091e96d058b2cd67c134a7df7c4c9bb99805f))
* per-engine query tab, Cmd+/ comment, disconnect, history persistence ([c1a73b9](https://github.com/suiflex/rdb/commit/c1a73b901e3357bc90bc687428ddc996585394ef))


### Bug Fixes

* **app:** keep query editor visible and typeable when empty ([e3f5fb8](https://github.com/suiflex/rdb/commit/e3f5fb8f454c9b92ec15267ffe77caf0ef8c7cca))
* **app:** render engine hint as placeholder, not seeded buffer text ([fefac4e](https://github.com/suiflex/rdb/commit/fefac4eb12b360255a71cd134b3ae56f47d19186))

## [0.8.0](https://github.com/suiflex/rdb/compare/v0.7.0...v0.8.0) (2026-07-09)


### Features

* **app:** ⌘\\ runs the current statement in a new tab ([b88124b](https://github.com/suiflex/rdb/commit/b88124b405f7efffb2d642f1f1445c126df3ded8))
* **app:** client-side column sort with header arrows ([5c55e91](https://github.com/suiflex/rdb/commit/5c55e918cbada44f43d3059178a6914defa7a37f))
* **app:** data grid and editor UX overhaul ([8942bf4](https://github.com/suiflex/rdb/commit/8942bf4f883a852dc6dc5e77a74b885b7acdd9dd))
* **app:** drag-to-reorder result columns ([4df638d](https://github.com/suiflex/rdb/commit/4df638d53bf7e5c34fb3631d4fdd1eb6d16e381f))
* **app:** draggable editor/results split ([f3322ca](https://github.com/suiflex/rdb/commit/f3322caa2412df3b87bfb863792ec955067f69f6))
* **app:** editable limit stepper with thousands grouping ([48792bb](https://github.com/suiflex/rdb/commit/48792bb6d9df78fbdffdf91dc9f9032f2944b819))
* **app:** per-column filter row with operator prefixes ([b83cc28](https://github.com/suiflex/rdb/commit/b83cc285087495d14c8e9734894ee3605c8e6a0b))
* **app:** result tabs — ⌘\\ opens a new result, ⌘⏎ replaces ([b60e235](https://github.com/suiflex/rdb/commit/b60e235c44e2cd59da188b6982d8f3a0d2908bb0))
* **app:** run the statement under the cursor on ⌘⏎ ([3540d0a](https://github.com/suiflex/rdb/commit/3540d0aac64ae3bdbd4d6a4d79671742bf4d38e6))
* **app:** schema/table/column autocomplete in the SQL editor ([658ce2e](https://github.com/suiflex/rdb/commit/658ce2ec1773f984bc2dd1cb4ccd0ac428350e09))
* **app:** show filter panel in SQL query tabs ([9a54e4a](https://github.com/suiflex/rdb/commit/9a54e4acf96d1103f07136ab6535b5a50c40cc65))


### Bug Fixes

* **app:** remove useless borrows in formatting call ([d9ed408](https://github.com/suiflex/rdb/commit/d9ed408edabfc2115156bc80084407c0b2c5c0d9))
* **app:** sticky result header and live per-column filtering ([1787838](https://github.com/suiflex/rdb/commit/178783894f4029de829f0bca58962dd4090ece25))
* **app:** useless borrows in formatting call in emiten mock ([70bcff0](https://github.com/suiflex/rdb/commit/70bcff0a0b9b302ed56690dd344a2e0ffd37a412))

## [0.7.0](https://github.com/suiflex/rdb/compare/v0.6.0...v0.7.0) (2026-07-09)


### Features

* **app:** add selected-cell inspector for full and JSON values ([0f1d3f8](https://github.com/suiflex/rdb/commit/0f1d3f8f7360427194f6942b275aa450699772af))
* **app:** make cell inspector resizable by dragging its top edge ([d01bd1d](https://github.com/suiflex/rdb/commit/d01bd1d306497e386552c916ef506d8abc98bc87))


### Bug Fixes

* **app:** bubble editor keys to window shortcut scope for run ([eadb051](https://github.com/suiflex/rdb/commit/eadb051be09826c0c4f16d18cbc7ea324293c154))
* **app:** reset last tab on close and clear grid on new tab ([a111ce7](https://github.com/suiflex/rdb/commit/a111ce7290460b130c40be66acb6ce86959ca37d))
* **app:** search icon, ⌘⏎ run, Postgres JSON cells, tab close/new ([4e91b9a](https://github.com/suiflex/rdb/commit/4e91b9aa8be4afc32beb27d3ef3e4343d491f7a2))
* **app:** use search SVG icon in search and filter fields ([ffa30d4](https://github.com/suiflex/rdb/commit/ffa30d45de70ad5448f474a261f5aaf9618471e9))

## [0.6.0](https://github.com/suiflex/rdb/compare/v0.5.0...v0.6.0) (2026-07-08)


### Features

* **app:** add Cassandra (CQL) driver and wire into UI ([c12237c](https://github.com/suiflex/rdb/commit/c12237c413376c72962f07856170c8ef0e51f962))
* **app:** add Cassandra engine badge and color ([25cef18](https://github.com/suiflex/rdb/commit/25cef1879b3d9a5fd0447035525cc95ebc84045d))
* **app:** add Cassandra option to the connection form ([5611a6e](https://github.com/suiflex/rdb/commit/5611a6ea98d4d8d65e0c28e7fca715bd78644120))
* **app:** add IconButton and SVG icons for caret/clock ([7c986a2](https://github.com/suiflex/rdb/commit/7c986a268bf75be284b11d837eede79f11c37f5f))
* **app:** add SQLite driver and wire into UI ([48cbcc9](https://github.com/suiflex/rdb/commit/48cbcc93abc6a5f51e38eccef3ecae4ce46f7b66))
* **app:** add SQLite driver and wire into UI ([800da02](https://github.com/suiflex/rdb/commit/800da02b65b496d622dea86380ac38aeb9e4caec))
* **app:** dispatch Cassandra driver via AnyDriver ([1a147db](https://github.com/suiflex/rdb/commit/1a147db9dab79ef98317b727a5a453f8d0f0db7b))
* **app:** macOS word and line motions in the query editor ([c68aedc](https://github.com/suiflex/rdb/commit/c68aedcc6af209073d4c65a7e566e1217f228059))
* **app:** native macOS window frame and rename to RDB ([4e13262](https://github.com/suiflex/rdb/commit/4e1326299a297e5e14494cb787cb95440c1a9ccf))
* **app:** parse Cassandra queries as CQL text ([7d73252](https://github.com/suiflex/rdb/commit/7d73252d0da54f58725dad9a964b4c5525e70eac))
* **app:** show connecting progress and errors on Connect ([d251207](https://github.com/suiflex/rdb/commit/d251207a32552beab87ef0295bb5a28cef8dccb7))
* **app:** TablePlus-grade query editor + inline CRUD ([9c1ec75](https://github.com/suiflex/rdb/commit/9c1ec75706e260c725eed9944631f56eec91f851))
* **app:** TablePlus-grade query editor + inline CRUD ([84c5f1c](https://github.com/suiflex/rdb/commit/84c5f1c40c4600d34ccc6bed042d509f8abe2cb7))
* **app:** use native OS window frame and rename to RDB ([6dc65b4](https://github.com/suiflex/rdb/commit/6dc65b4d3e39b0589d91a2b4e61ed38f50226bba))
* **app:** wire Cassandra into ports, browsing, editor and tree ([838e380](https://github.com/suiflex/rdb/commit/838e3805f60d5e5198a2bf838f30dcb74b629d8c))


### Bug Fixes

* **app:** compile Slint on a large-stack thread for Windows ([9ff8dc5](https://github.com/suiflex/rdb/commit/9ff8dc57955d2ac0700502ada463c8f627a649e3))
* **app:** connection-form + editor UX (dropdowns, connect feedback, motions, icons) ([b3b36ac](https://github.com/suiflex/rdb/commit/b3b36ac4d1ff2b14c8569161fb057c3a6c1b10f2))
* **app:** replace blank ComboBox with a custom SelectBox ([5c8a40f](https://github.com/suiflex/rdb/commit/5c8a40f6989cd1d2e55bcead97997bfbbb6d9b3e))
* **app:** use SVG icons for header history and theme toggle ([1bbd0d8](https://github.com/suiflex/rdb/commit/1bbd0d86d0b0e2b533ce850f821652b686458566))

## [0.5.0](https://github.com/suiflex/rdb/compare/v0.4.0...v0.5.0) (2026-07-04)


### Features

* **app:** chart view for tabular results ([efa50a7](https://github.com/suiflex/rdb/commit/efa50a7ceeac87987627f8c5642dbdef37137266))
* **app:** column visibility popup ([c662818](https://github.com/suiflex/rdb/commit/c6628185c51f832a180d463cba3fb9565f3516a9))
* **app:** copy results to clipboard and CSV export ([bd01126](https://github.com/suiflex/rdb/commit/bd0112673ea31d29111d7e35ce07ac1265126757))
* **app:** Explain button runs EXPLAIN on the editor SQL ([892ab9a](https://github.com/suiflex/rdb/commit/892ab9ab9bc9aca7644c1f3dff04dd09eb7256ba))
* **app:** filter panel uses real columns and operators ([8857f45](https://github.com/suiflex/rdb/commit/8857f458fe992701159fd4dd768e0a30d3472bee))
* **app:** implement dark palette behind Theme.dark ([337c3ab](https://github.com/suiflex/rdb/commit/337c3abb10b01e39f2851b0704be0e79d3d3a6c4))
* **app:** indexes view populated from engine catalogs ([c836956](https://github.com/suiflex/rdb/commit/c8369561ccbc6d4ee3ad64ccfa49ffca68d6376a))
* **app:** live query history and sidebar mode content ([40e0fe6](https://github.com/suiflex/rdb/commit/40e0fe66f7c8ad7411145bc7bad7cf2a119ac96a))
* **app:** picker import and backup actions ([4ed0d4b](https://github.com/suiflex/rdb/commit/4ed0d4b216cac5ad5f16e17533baf3095ec2d7e8))
* **app:** quick-test connection from picker detail pane ([216ed1b](https://github.com/suiflex/rdb/commit/216ed1ba7baa2890b9b0dd0739a3b9c4b657d388))
* **app:** Run Selection executes the statement under the cursor ([debe716](https://github.com/suiflex/rdb/commit/debe716ffff1d0c51cb0cadbde71f1829cc7487c))
* **app:** schema switcher modal ([d9944e5](https://github.com/suiflex/rdb/commit/d9944e5d185957b67bcbad7ce106fe37582e28a8))
* **app:** SQL Format button ([3c197aa](https://github.com/suiflex/rdb/commit/3c197aa53c4167ddbd4e43565f890e1b9265dad4))


### Bug Fixes

* **app:** limit control drives set-limit and shows the real value ([0114c8a](https://github.com/suiflex/rdb/commit/0114c8a63b22705f1ad08c49e3323b60f4d382ba))
* frontend audit — wire dead controls, implement missing features ([16f131d](https://github.com/suiflex/rdb/commit/16f131d68fabb5e2a3d53ebe1d2ca7c6f728c294))
* **ui:** distinct disabled state for SecondaryButton ([7d0016d](https://github.com/suiflex/rdb/commit/7d0016d98be797d43fbecc9d077387291e6949d1))
* **ui:** sidebar plus button opens a new query tab ([1035b79](https://github.com/suiflex/rdb/commit/1035b79cda9df95e42a2a73df9edb4661bf3c4a3))
* **ui:** titlebar aux buttons do what they say ([fd54fa2](https://github.com/suiflex/rdb/commit/fd54fa20efd5fdddab937b8deff0b956b3ed5cae))

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
