# Changelog

## RDB: [0.42.0](https://github.com/suiflex/rdb/compare/v0.41.0...v0.42.0) (2026-08-26)


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

## RDB: [0.41.0](https://github.com/suiflex/rdb/compare/v0.40.0...v0.41.0) (2026-08-22)


### App Features

* **app:** add word and line delete primitives to the editor ([aa1ba3e](https://github.com/suiflex/rdb/commit/aa1ba3ed92d8bcc49855c5666c0f2ccfc301f50d))
* **app:** complete field names and operators in a Mongo filter document ([3eab3f7](https://github.com/suiflex/rdb/commit/3eab3f77d7caf4c52a42e556188e2d715305fe26))
* **app:** copy pending grid edits as SQL ([8cbd291](https://github.com/suiflex/rdb/commit/8cbd291884f6dc633c9f9341aff27d686165827d))
* **app:** derive the theme from the OS appearance ([48e8f1a](https://github.com/suiflex/rdb/commit/48e8f1a1621201c4968dc8460c6431f987bdfc52))
* **app:** give the theme and settings buttons real icons ([6b36524](https://github.com/suiflex/rdb/commit/6b3652463cce6798daa2ba87430da4c7fbcd79af))
* **app:** show an autocomplete popup in the Mongo filter box ([30c6b98](https://github.com/suiflex/rdb/commit/30c6b98ac6124dc3e13f89586f47a3d4a7f2cf91))
* **core:** carry a client identity for drivers to announce ([efe3d26](https://github.com/suiflex/rdb/commit/efe3d265e639ae2c403c087d295735765dee5a5a))


### Bug Fixes

* **app:** honor Option+Delete and Command+Delete in the editor ([378b877](https://github.com/suiflex/rdb/commit/378b877f0ce3c0e735a1a8a24bd5636b66b55e8b))
* **app:** keep editor folds with the tab they belong to ([901a4ea](https://github.com/suiflex/rdb/commit/901a4ea61aa4faa6746b13502a07a35862c764fc))
* **app:** keep table tabs across a schema switch ([222ba09](https://github.com/suiflex/rdb/commit/222ba0917e5e35363e1b2e5adc52e73e2a0793ad))
* **app:** keep table tabs across disconnect and reconnect ([7b145a8](https://github.com/suiflex/rdb/commit/7b145a8884be3787cea6b1354a633403142f8df7))
* **app:** make Cancel abort the server statement, not just the task ([24e1028](https://github.com/suiflex/rdb/commit/24e10286d9c84e60184de64444a81bfa64659a88))
* **app:** stop an unreadable password looking like no password ([bb43448](https://github.com/suiflex/rdb/commit/bb434489cd2ea72fe819bf912d74ff930fc180ff))
* **app:** test an existing connection with its saved password ([f3cb1a6](https://github.com/suiflex/rdb/commit/f3cb1a68545e2bb2552a0a3af2259a2475bc1325))

## RDB: [0.40.0](https://github.com/suiflex/rdb/compare/v0.39.0...v0.40.0) (2026-08-19)


### App Features

* **app:** surface MariaDB and Valkey in the connection form ([6c2720e](https://github.com/suiflex/rdb/commit/6c2720e1f2f7b9653830029cd381d60bd04025f6))
* **connstore:** add MariaDB and Valkey as aliased engines ([f865878](https://github.com/suiflex/rdb/commit/f8658783b16ceae7d2705d10dd53e3635114bc7c))
* **tunnel:** implement native SSH tunneling for database connections ([20d8164](https://github.com/suiflex/rdb/commit/20d8164f122aca790ebc6276b2a7037caf2218ec))


### Bug Fixes

* **app:** add MariaDB and Valkey to the picker's "Works with" row ([8595325](https://github.com/suiflex/rdb/commit/85953253ecab78bd4abe36397dc482836e982a6e))
* **app:** allow cancelling connect while switching connections ([fff3d02](https://github.com/suiflex/rdb/commit/fff3d0267a85185a8d7b3d182cb5a9c085b8e0da))
* **app:** keep query fold anchored through line inserts/deletes ([ab53d0c](https://github.com/suiflex/rdb/commit/ab53d0ce4a53bfdbb53a4d1750154a7a02d63983))
* **app:** scroll the editor horizontally to follow the caret ([b89c6fa](https://github.com/suiflex/rdb/commit/b89c6fa9c583f4a5d1986da9e797310785e881bc))
* **app:** stop the editor gutter clipping after scrolling back left ([1f2e2e9](https://github.com/suiflex/rdb/commit/1f2e2e95297e835d2833f86587903c233c6b1344))
* **app:** use the close icon for the cancel-connect button ([b16563b](https://github.com/suiflex/rdb/commit/b16563b6f9f0d3eaff434540d13686f6d1545365))

## RDB: [0.39.0](https://github.com/suiflex/rdb/compare/v0.38.0...v0.39.0) (2026-08-15)


### App Features

* **app:** restore query tabs at startup, not on first connect ([061d28c](https://github.com/suiflex/rdb/commit/061d28c7b1038197de9e1075943e5ea36e4d89b8))


### Bug Fixes

* **app:** keep query tabs inside RDB_STORE_DIR ([952ce08](https://github.com/suiflex/rdb/commit/952ce08b197f0b369a4136ac1cf2e7a9399bcc75))
* **app:** persist both panes of a split workspace ([59e722a](https://github.com/suiflex/rdb/commit/59e722ab9993ea695068da826c35a6206af7c5b6))
* **app:** stop Enter appending a newline in the details panel ([ca9e855](https://github.com/suiflex/rdb/commit/ca9e85520fbf87311cf280a43be6b823a7700fde))
* **app:** treat a Homebrew cask install as package-managed ([8d8f270](https://github.com/suiflex/rdb/commit/8d8f2705dff96ccc3d0b76da700e8809a0594faa))


### Performance Improvements

* **app:** build the displayed grid in one pass ([49e0681](https://github.com/suiflex/rdb/commit/49e0681deddb456f822aa6a871b5c4bf33b2632a))
* **app:** share result views instead of deep-copying them ([a6061f8](https://github.com/suiflex/rdb/commit/a6061f8bbb619e431b2d74c23eb57f97e4e9f83e))

## RDB: [0.38.0](https://github.com/suiflex/rdb/compare/v0.37.0...v0.38.0) (2026-08-14)


### App Features

* **app:** let the query error highlight be turned off ([ec7766b](https://github.com/suiflex/rdb/commit/ec7766b8293a327c10e55984d04abf4e34bb40c8))
* **app:** mark the error line number in the editor gutter ([40a7179](https://github.com/suiflex/rdb/commit/40a7179f9bf7f089fae34be8e8cb78b648e4d79b))
* **app:** squiggle the token a query error points at ([c098b33](https://github.com/suiflex/rdb/commit/c098b335c8527b8118822395975e1e5661a8ec8f))
* **app:** tint the whole failing statement, not just its error line ([980f525](https://github.com/suiflex/rdb/commit/980f525f27317e2694e91fdd69a3287ab29868e0))


### Bug Fixes

* **app:** confirm connection deletion ([bcb73ce](https://github.com/suiflex/rdb/commit/bcb73ce63f8f367862a94d0cd861b908a8bea8de))
* **app:** handle empty completion aliases ([d429489](https://github.com/suiflex/rdb/commit/d42948930bee99ccb4440b2c97c97b215ca8d9a0))
* **app:** harden MySQL connection setup ([9b11cd3](https://github.com/suiflex/rdb/commit/9b11cd33a5fef788e8f5f1beffb8094714ed7a3e))
* **app:** hide the internal error marker from query errors ([42ced3a](https://github.com/suiflex/rdb/commit/42ced3af37b6dbd1f83747e47e33312e3428f1fa))
* **app:** highlight the error line when Run sends one statement ([75232de](https://github.com/suiflex/rdb/commit/75232de744109a602dc21e7977e43f85a79e0f1b))
* **app:** stop hidden form blocking clicks ([a3698fa](https://github.com/suiflex/rdb/commit/a3698faae214608babe2ea4d5a042e731d1bd1d9))
* **drivers:** honor user SSL preferences ([992842d](https://github.com/suiflex/rdb/commit/992842dcd83b9d2b81ef43cf3ea2deba4486d095))

## RDB: [0.37.0](https://github.com/suiflex/rdb/compare/v0.36.0...v0.37.0) (2026-08-13)


### App Features

* **app:** add auto table alias toggle to SQL completion ([9ba49b1](https://github.com/suiflex/rdb/commit/9ba49b14a160e9320b7b00488d80f711b67d3e8f))
* **app:** filter connections by group and env too ([4677ef9](https://github.com/suiflex/rdb/commit/4677ef96ec42a1bad64e516b3b122b399194eb84))
* **app:** group Settings Appearance tab into labeled sections ([f88a055](https://github.com/suiflex/rdb/commit/f88a05568ebb2300e46088d6c090de8ede9a8e4a))
* **app:** highlight query error line in the SQL editor ([8c5aa39](https://github.com/suiflex/rdb/commit/8c5aa399103c3c20aac08b84b7f0af934403c92a))
* **app:** match sidebar's group/env filter in command palette search ([3175e63](https://github.com/suiflex/rdb/commit/3175e633052d8dd42c85a43550b4ac28806c3256))
* **app:** show group/subgroup under connections in command palette ([4e56d8c](https://github.com/suiflex/rdb/commit/4e56d8c485a6254137e6a04baee74a09923c54a2))


### Bug Fixes

* **app:** actually center the overflow-menu dots in their circle ([cd0604f](https://github.com/suiflex/rdb/commit/cd0604fd57ce1114d9b723bef8b652c1c19ba3de))
* **app:** align group chevron and center the overflow-menu hover ([361f8b4](https://github.com/suiflex/rdb/commit/361f8b45b3a8f4a56d2392425b2445ce09e582f8))
* **app:** close new-connection modal on backdrop click ([0e3d128](https://github.com/suiflex/rdb/commit/0e3d12812d2c1cb1b0726715537b4423ed7e3676))
* **app:** fix blank grid cell editor for compact JSON values ([26641b6](https://github.com/suiflex/rdb/commit/26641b664a14ac03e5dddc2fbb794931aa7b3006))
* **app:** fix stale row/col and desynced text in grid cell editor ([1831359](https://github.com/suiflex/rdb/commit/1831359366906eca86cdaf55b2e469d7be5fec5e))
* **app:** fold pasted smart punctuation to ASCII ([cdfd507](https://github.com/suiflex/rdb/commit/cdfd507eae496c434e59b465b6f439d488c3adb7))
* **app:** force cell editor viewport to top-left, raise height cap ([b6c5b24](https://github.com/suiflex/rdb/commit/b6c5b2442c2ff0e5dcc779a9e91610ca7e94cf68))
* **app:** highlight COMMIT and ROLLBACK in the SQL editor ([137a211](https://github.com/suiflex/rdb/commit/137a211792fb3b11e3d6645e5c6f4c3ebaa8ed8b))
* **app:** highlight the correct line for multi-statement errors ([6bd7546](https://github.com/suiflex/rdb/commit/6bd75461ef0ce06373d2abe9a107cb28cc359e92))
* **app:** make auto-table-alias toggle actually work ([c2b0579](https://github.com/suiflex/rdb/commit/c2b05793804c711473093c0caed426ea97a81a3a))
* **app:** set editing_value before editing_row/col to fix cursor jump ([b10fea3](https://github.com/suiflex/rdb/commit/b10fea3547a5c7718756be34046653228d000bb9))
* **app:** shorten Auto table alias helper, align shortcuts with Done ([9f6e7b6](https://github.com/suiflex/rdb/commit/9f6e7b6bdaaf099e9120c4df599f555d802120cc))
* **app:** show social banner on selected-connection view too ([ef26351](https://github.com/suiflex/rdb/commit/ef26351e1306314f02e1c083ef2a5295cc07fe15))
* **app:** split grid cell editor into inline box vs centered modal ([8d9e7a5](https://github.com/suiflex/rdb/commit/8d9e7a5bd164c91d265005981f265d1e28ef5404))
* **app:** stop truncating long values in grid cell editor ([ea1a918](https://github.com/suiflex/rdb/commit/ea1a9186f2ee4b128a9b7a5904e0d41c15ee773f))
* **app:** strip orphaned variation selectors in pasted text ([789dd91](https://github.com/suiflex/rdb/commit/789dd913185f107677cbab0ef615081e0432c0f7))
* **app:** strip PUA and replacement chars from pasted text ([b7e9262](https://github.com/suiflex/rdb/commit/b7e92623bf6bda0e8e5aba77aa755d5e73901921))
* **app:** suppress irrelevant SQL completions ([e83843d](https://github.com/suiflex/rdb/commit/e83843dfb7a925fdf57466ce5b06269d795ae87b))
* **app:** use English wording for connection detail group/subgroup ([eeb72f7](https://github.com/suiflex/rdb/commit/eeb72f738dc71447eca1de2c31afdf3192bd62dc))
* **app:** use TextEdit for the inline grid cell editor ([567a67c](https://github.com/suiflex/rdb/commit/567a67c4b600d4f23bece50fc5f149531fa65541))


### Performance Improvements

* **app:** fetch cross-schema completion nodes concurrently ([dd72fe5](https://github.com/suiflex/rdb/commit/dd72fe570f234a0d28f4be1d71e53678aefb158e))

## RDB: [0.36.0](https://github.com/suiflex/rdb/compare/v0.35.0...v0.36.0) (2026-08-12)


### App Features

* **app:** extend the editor selection with shift-click ([681d20c](https://github.com/suiflex/rdb/commit/681d20c69ccb10cf69436ee34a7b69e991d5e8ba))
* **app:** outline top-level connection groups, confirm group delete ([efdfb85](https://github.com/suiflex/rdb/commit/efdfb85e73184a70ad5bd1b8b17399bae198f26e))
* **app:** sanitize invisible and exotic whitespace in the editor ([9600c94](https://github.com/suiflex/rdb/commit/9600c946ed958f3a694ad7201a85c35510ea6a96))


### Bug Fixes

* **app:** align Grid/Tree toggle with its results-toolbar siblings ([5016c37](https://github.com/suiflex/rdb/commit/5016c3757fe54a9175ef1c636bc7e678aa3b98b2))
* **app:** align Mongo result view toggles ([908b545](https://github.com/suiflex/rdb/commit/908b545553e612caca3c1769a7e36be93dfb5a69))
* **app:** center Grid/Tree with explicit y, not nested-layout stretch ([8e3148b](https://github.com/suiflex/rdb/commit/8e3148b1212711c0b8c5d71107421f18037300a4))
* **app:** give UnderlineSegment a fixed height ([ea93e29](https://github.com/suiflex/rdb/commit/ea93e29ee1b2527c3883b3f5f81bac1c35c5e38e))
* **app:** retain query tabs on database switch ([5b66079](https://github.com/suiflex/rdb/commit/5b660793fa7f040ebb453bb66ec43a01498a1897))
* **app:** show Mongo query results in tree view ([a141def](https://github.com/suiflex/rdb/commit/a141def4c04ec59e7e56acd78ddabc7caadd2383))
* **app:** stop doubling "connection failed:" and clipping the message ([e1feec7](https://github.com/suiflex/rdb/commit/e1feec7afdac16434275fdecacb1c8b1e76e239c))
* **app:** stop Redis connections silently defaulting to TLS ([a3af387](https://github.com/suiflex/rdb/commit/a3af3874e4008a09d1a8e4df82f9274418229d74))
* **app:** stop the Grid/Tree wrapper stretching over Copy/Export/Chart ([4dca910](https://github.com/suiflex/rdb/commit/4dca910cceb3383f430e6f330466c6307565b378))
* **app:** tighten nested group spacing ([b737311](https://github.com/suiflex/rdb/commit/b7373119ecf397b05de7654096430f15559cebd8))

## RDB: [0.35.0](https://github.com/suiflex/rdb/compare/v0.34.1...v0.35.0) (2026-08-10)


### App Features

* **app:** complete cross-schema tables by their own name ([17912bd](https://github.com/suiflex/rdb/commit/17912bdf1756ae04b8ea1ed0e013d90e579e874d))
* **app:** mongo chained modifiers after a closed call ([8ae4170](https://github.com/suiflex/rdb/commit/8ae4170f64f5d2f3621192edb766f6d2a6b6ea4d))
* **app:** mongo field, operator and stage completion ([98f43a9](https://github.com/suiflex/rdb/commit/98f43a92cba483f969aa24bd5c2ac00a65e3700a))
* **app:** new-tab button names the connected engine's query language ([4a2fbed](https://github.com/suiflex/rdb/commit/4a2fbedc04eb703bde519d66fcb33a9ebed8fdc9))
* **app:** seed mongo completion with sampled fields ([3fdff66](https://github.com/suiflex/rdb/commit/3fdff669956e8009ec2eefa17c0b3330f4e690e9))
* **app:** show query latency in seconds past one second ([af84c91](https://github.com/suiflex/rdb/commit/af84c9196f2e146cbfd4455ad0221b6970c2205e))


### Bug Fixes

* **app:** mongo db.&lt;collection&gt;. offers methods over sampled fields ([d413066](https://github.com/suiflex/rdb/commit/d4130666c014177d85ed299cf0480e90fea94047))
* **app:** refresh completion after a schema/database switch ([36f90e0](https://github.com/suiflex/rdb/commit/36f90e0107a2a422f2ed920d6d0852d074332358))

## RDB: [0.34.1](https://github.com/suiflex/rdb/compare/v0.34.0...v0.34.1) (2026-08-10)


### Bug Fixes

* **app:** enter tokio runtime before Slint/startup init ([34aa5c2](https://github.com/suiflex/rdb/commit/34aa5c2ca4123f05181f947bd2242aaddeb4f058)), closes [#195](https://github.com/suiflex/rdb/issues/195)

## RDB: [0.34.0](https://github.com/suiflex/rdb/compare/v0.33.0...v0.34.0) (2026-08-09)


### App Features

* **app:** add supported-engines trust-badge row to empty state ([50b84fc](https://github.com/suiflex/rdb/commit/50b84fcd59e54a0bf2a46343ceca03362ae4cfc5))
* **app:** give query tabs a rounded-chip look ([c477a6e](https://github.com/suiflex/rdb/commit/c477a6e25a85b179a5477c3a8eb77a2d9d625715))
* **app:** give workspace sidebar a floating-card look ([8479fe9](https://github.com/suiflex/rdb/commit/8479fe92bf964ab340177464dab86275fdf2e866))
* **app:** nested connection groups, tab/sidebar polish, tab-collision fix ([3fdabb2](https://github.com/suiflex/rdb/commit/3fdabb289e93d5072d3c4c257e77660998a61b6f))
* **app:** show brand logo before engine name in the Engine dropdown ([f2e7cb9](https://github.com/suiflex/rdb/commit/f2e7cb910c1cf3fdb8f5ddca5f31c67bc7d30947))
* **app:** support nested connection groups with a guided picker ([090e053](https://github.com/suiflex/rdb/commit/090e0538e3e9ace370fbf09d684fbdf26c0713a9))
* **app:** upgrade sidebar db icons, add ClickHouse, SQL Server fallback ([b6ae057](https://github.com/suiflex/rdb/commit/b6ae057c99b1b275f4f34c7773700c92699d94b2))
* **app:** wire Clickhouse into AnyDriver dispatch and UI ([4222386](https://github.com/suiflex/rdb/commit/422238651917a8520e33d49d68706f86b54f27c0))
* **app:** wire Mssql into AnyDriver dispatch and UI ([6766181](https://github.com/suiflex/rdb/commit/676618105216969d342bbddfe0abb4da34ae528e))
* SQL Server + ClickHouse drivers, query-language refactor ([bda143f](https://github.com/suiflex/rdb/commit/bda143fdd85c875f449243362a3d616a921b1307))


### Bug Fixes

* **app:** center EngineLogo vertically in the Engine dropdown ([983c1a0](https://github.com/suiflex/rdb/commit/983c1a0abf012c39e774026a7d59f408b2e4589e))
* **app:** seed tab counter from restored ids to avoid collisions ([0f85a58](https://github.com/suiflex/rdb/commit/0f85a58e28cc9fb657747d802146c2429ace1319))
* **app:** tighten sidebar floating-card gutter ([9df3af9](https://github.com/suiflex/rdb/commit/9df3af987e65f4bfc7ab244efe52e24bf11c511c))
* **driver-cassandra:** accept Query::Cql instead of Query::Sql ([17c1a12](https://github.com/suiflex/rdb/commit/17c1a12fae4d85b65545add937459b42bdbf5079))

## RDB: [0.33.0](https://github.com/suiflex/rdb/compare/v0.32.1...v0.33.0) (2026-08-07)


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

## RDB: [0.32.1](https://github.com/suiflex/rdb/compare/v0.32.0...v0.32.1) (2026-08-07)


### Bug Fixes

* **app:** auto-close matching bracket/quote pairs in editor ([2bb1e84](https://github.com/suiflex/rdb/commit/2bb1e846196792e02a87a682f3d5ff568b634618))
* **app:** debounce column filter query on keystroke ([28261ad](https://github.com/suiflex/rdb/commit/28261ad4a31c91bbaf33128484b31d93a0b3f8d5))
* **app:** keep inline cell editor sized to live input ([7a5d08c](https://github.com/suiflex/rdb/commit/7a5d08cd08e84ce0a160cf9542b1ef9841bdd213))
* editor auto-pair, cell edit sizing, filter debounce, cask Gatekeeper ([a2b283c](https://github.com/suiflex/rdb/commit/a2b283c314f35eafd5ecf609bcee7aa859d23849))

## RDB: [0.32.0](https://github.com/suiflex/rdb/compare/v0.31.0...v0.32.0) (2026-08-05)


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

## RDB: [0.31.0](https://github.com/suiflex/rdb/compare/v0.30.0...v0.31.0) (2026-08-03)


### App Features

* **app:** consistent group UI across the picker and ⌘O modal ([4984f6c](https://github.com/suiflex/rdb/commit/4984f6c609d986931242a523464cb17bed9f3d4d))
* **app:** group query History by date, show its connection ([8760f19](https://github.com/suiflex/rdb/commit/8760f19d088f19eaf0f3413302c6d589a1be861e))
* **app:** let connections be assigned to a sidebar group ([c7c6975](https://github.com/suiflex/rdb/commit/c7c6975afe708abc527f942831d9af2f08676555))
* **app:** make Group a real picker, manageable from the sidebar ([22b023b](https://github.com/suiflex/rdb/commit/22b023ba216a28a6950ca08b0be6f0bebd760336))
* **app:** show the active connection's env tag in the header ([cbc3977](https://github.com/suiflex/rdb/commit/cbc3977f2e86b53daed84425c3df12555a2987a4))
* **app:** suggest common date/aggregate functions in autocomplete ([ab385dc](https://github.com/suiflex/rdb/commit/ab385dc2c0e3d6a628a650972b52dc15348eef96))
* **app:** tag saved connections with a colored environment badge ([cab78df](https://github.com/suiflex/rdb/commit/cab78df66c6c066fa3cbab20d5ee2db17999fb72))


### Bug Fixes

* **app:** actually stretch the group menu button to the right ([40cf3a0](https://github.com/suiflex/rdb/commit/40cf3a066d5cab300399265a1ff26f0cafcbe77a))
* **app:** add breathing room around the dialog's section dividers ([ac8d18f](https://github.com/suiflex/rdb/commit/ac8d18f4ae7fe3ed57bc2c84ba383f942b9da028))
* **app:** add missing SQLite case to URL placeholder ([b085729](https://github.com/suiflex/rdb/commit/b085729f2915278fa80131ca16b857ae99cf0155))
* **app:** add tooltips to tab rename/split icons ([d19c7c3](https://github.com/suiflex/rdb/commit/d19c7c30395b308c50a113a9c211afd3495cf70b))
* **app:** align group-header and result-tab labels with their icons ([093df7a](https://github.com/suiflex/rdb/commit/093df7ac7bb87672106bf8cd8b5457993f878d77))
* **app:** fix group menu icon, add drag-to-group ([0927398](https://github.com/suiflex/rdb/commit/092739877ef4f6252c4c0722bccf6e9f12ef26f8))
* **app:** Format/Explain act on the full statement, not one line ([6c03cb1](https://github.com/suiflex/rdb/commit/6c03cb11a7e0242e33d3c43c5fbcb9842e48cc41))
* **app:** group menu opens at the wrong spot, dots misaligned ([a3cca2b](https://github.com/suiflex/rdb/commit/a3cca2b2e403e98b749a5a5d5f2a5f9e0e3e3641))
* **app:** History badge shows the connection's actual color ([b357e8d](https://github.com/suiflex/rdb/commit/b357e8dc5d4591e7145899412d75fee34c31e978))
* **app:** preserve active tab when moving it between panes ([fa48ab5](https://github.com/suiflex/rdb/commit/fa48ab50614958c691f4f6c46d0cf2f8df794950))
* **app:** remove the visible gap between rows in the same card ([a7ddf0a](https://github.com/suiflex/rdb/commit/a7ddf0a00ba1fd646fad9e1c53a7a0a6f176bccb))
* **app:** reuse an existing group across case differences ([1f576bf](https://github.com/suiflex/rdb/commit/1f576bf997630b2b857c651488491f73d4f144da))
* **app:** shift width from detail panel to connection list ([c450fec](https://github.com/suiflex/rdb/commit/c450fecde5e1eaa85ede33868464cf8aeb8b325d))
* **app:** show a visible menu button on group headers ([3c9c592](https://github.com/suiflex/rdb/commit/3c9c5929970f03f8241e6d6e3cc60f0ee2d2c81e))
* **app:** show environment pill in the connection pickers ([748f9d4](https://github.com/suiflex/rdb/commit/748f9d494356bc72ebbdc3ae209e56c9419d3036))
* **app:** show per-engine URL sample and shorten ENV label ([8b6e4a7](https://github.com/suiflex/rdb/commit/8b6e4a755cfa9bc1609a66ccea35c3f92f7e52f1))
* **app:** tag History rows by driver, refresh after ⌘\\ and split-run ([ee4042c](https://github.com/suiflex/rdb/commit/ee4042ce3732569b9b9ebabf87be5f272ae18559))
* **app:** widen connection list, compact the detail panel ([9c41e13](https://github.com/suiflex/rdb/commit/9c41e13d2580caa62a0fa4104b36e04a54d173ef))

## RDB: [0.30.0](https://github.com/suiflex/rdb/compare/v0.29.0...v0.30.0) (2026-08-01)


### App Features

* **app:** add query toolbar tooltips ([8a763a6](https://github.com/suiflex/rdb/commit/8a763a6ec7b313323fb39c4ab9cc3875a62ed6e4))
* **app:** badge document tabs with their source connection ([d60d9ac](https://github.com/suiflex/rdb/commit/d60d9acd041dd9ffea6e26cea3c3d942db8c641a))
* **app:** badge result tabs with their source connection ([ec2710f](https://github.com/suiflex/rdb/commit/ec2710fcb5b31930eaf6c9c49c094cabb014c819))
* **app:** in-app Restart to Update for direct-download installs ([7ccfb7c](https://github.com/suiflex/rdb/commit/7ccfb7caac812e44ca875453d4f9d15110844116))
* **app:** v0.30.0 prep — connection badges, self-update, UX fixes ([d02f5e7](https://github.com/suiflex/rdb/commit/d02f5e788e3ee870846c79c2eb7001ded8e05ae7))


### Bug Fixes

* **app:** add safe run-new-tab action ([6024e31](https://github.com/suiflex/rdb/commit/6024e316ef1e80855bd731252545b0b64ad41fd9))
* **app:** align shortcuts with active pane and platform ([93d57a2](https://github.com/suiflex/rdb/commit/93d57a23cb0e96da2978e30d67dfd2224bf1d0fc))
* **app:** export shortcut labels to Rust ([d6b5ed0](https://github.com/suiflex/rdb/commit/d6b5ed03d7d762688d656191686e7314fb6d6815))
* **app:** format and explain active query line ([976c0be](https://github.com/suiflex/rdb/commit/976c0be4d1db8b88a6b3d68931d8faafcbb3ae34))
* **app:** handle shifted reconnect shortcut ([93a26df](https://github.com/suiflex/rdb/commit/93a26dff65bdf2030ff349c4410fc5d6bfd10181))
* **app:** keep active-line formatting on one line ([f0656e3](https://github.com/suiflex/rdb/commit/f0656e34bd16f81a2e6aa58b375baa87c9e07799))
* **app:** register open shortcuts with native menu ([95e43ef](https://github.com/suiflex/rdb/commit/95e43ef7244d10479c9a4f4532a44754134dc48a))
* **app:** remove redundant run selection button ([a64963d](https://github.com/suiflex/rdb/commit/a64963d765d0f4d325a4bcbcac6358335d842182))
* **app:** render each connection's custom accent color ([004f985](https://github.com/suiflex/rdb/commit/004f985e5d4dd67296ed2d042d03313b6ff5a2f8))
* **app:** restore query tab rename controls ([43768ba](https://github.com/suiflex/rdb/commit/43768baa759ba53da8e5e0d461ecd654b7d1971f))
* **app:** route command backslash to active query ([2eb13ab](https://github.com/suiflex/rdb/commit/2eb13ab33505b75f7db368b3f7c8194cf29a40e0))
* **app:** scroll result chips and drop the "Result" word ([68dc640](https://github.com/suiflex/rdb/commit/68dc640fc3bce647e991726235ad89feadf143ec))
* **app:** use close icon in update dialog ([0c86540](https://github.com/suiflex/rdb/commit/0c86540c5f24eb6982e7e92b7b1fe5d5d5450be0))
* **app:** use each connection's custom color for tab/result badges ([e27d840](https://github.com/suiflex/rdb/commit/e27d8405e9c32dfd8afe0f81904c5006cdc19d91))

## RDB: [0.29.0](https://github.com/suiflex/rdb/compare/v0.28.0...v0.29.0) (2026-07-31)


### App Features

* **app:** v0.29.0 prep — split-pane, editor, tabs, mongo fixes ([#164](https://github.com/suiflex/rdb/issues/164)) ([be408f6](https://github.com/suiflex/rdb/commit/be408f6690a8af9334e759e1af5462b286d01367))

## RDB: [0.28.0](https://github.com/suiflex/rdb/compare/v0.27.0...v0.28.0) (2026-07-30)


### App Features

* **app:** add NoSQL collection-limit setting for MongoDB sidebar ([bb27c70](https://github.com/suiflex/rdb/commit/bb27c70f0f9876d6aa1006294b355aa5cd100f58))
* **app:** connection picker affordances ([541f6ad](https://github.com/suiflex/rdb/commit/541f6ad4dd12905530455459c70a4faf45dee1e7))
* **app:** make autocomplete engine-aware for MongoDB ([f22f535](https://github.com/suiflex/rdb/commit/f22f53591e22e2b46d281e833a5d8cfa4830cbfa))
* **app:** suggest MongoDB collections after db. ([23bcdf9](https://github.com/suiflex/rdb/commit/23bcdf929cbc95f2bff45b26ac1b7e4bc030137e))
* **app:** suggest MongoDB methods after db.&lt;collection&gt; ([5e088f0](https://github.com/suiflex/rdb/commit/5e088f078f91a16211b04e386d343601bf79bf7f))


### Bug Fixes

* **app:** balance the New Connection button glyph and height ([7674689](https://github.com/suiflex/rdb/commit/7674689accc5ad991222571fa28ea7dadf6794ff))
* **app:** cancel the in-flight query on disconnect ([c98f1d6](https://github.com/suiflex/rdb/commit/c98f1d6ffd14f5cba130526635df0d361df66b02))
* **app:** clip filter field text to its box ([ae49fb6](https://github.com/suiflex/rdb/commit/ae49fb6b5a11fe880132bbae5806d5fd5e292748))
* **app:** make large value previews wrap and scroll ([3b45d44](https://github.com/suiflex/rdb/commit/3b45d4417540484b1a3365ac273b625b9249cb07))
* **app:** make the result message scrollable ([985a32c](https://github.com/suiflex/rdb/commit/985a32ca6baf807f2ac57114e7e8ea8d0a8ddedb))
* **app:** match New Connection hover colour to the other footer buttons ([d965816](https://github.com/suiflex/rdb/commit/d965816dad5a6e64ce4b46e53b56b5afc858c774))
* **app:** run queries and pings without holding the driver mutex ([4fb65ca](https://github.com/suiflex/rdb/commit/4fb65cace8b38a693fedda8c5435f178c8705fee))
* **app:** run the statement under the caret at a semicolon boundary ([7e27bd4](https://github.com/suiflex/rdb/commit/7e27bd4e045ecaff870b4aef55925bb331ed3e4b))
* **app:** scope MongoDB sidebar to the connection's selected database ([1195e84](https://github.com/suiflex/rdb/commit/1195e84569963d1169356c38480fd29c85f03313))
* **app:** scroll filter field to the caret instead of clipping the tail ([b60205f](https://github.com/suiflex/rdb/commit/b60205f8e38c35bfca0acdf16dee9cdf06395c17))
* **app:** scroll the filter input to the caret so long text stays visible ([d29a0be](https://github.com/suiflex/rdb/commit/d29a0beeed248f645f2159522397923e89c81417))
* **app:** stop the filter field growing with its text ([0b685b2](https://github.com/suiflex/rdb/commit/0b685b23ea273bafe89bd82493e623c2ae6f162b))
* **app:** vertically center the New Connection footer button ([aaf3cc4](https://github.com/suiflex/rdb/commit/aaf3cc4a21d79852dfb1a5f276ea3f52659de792))

## RDB: [0.27.0](https://github.com/suiflex/rdb/compare/v0.26.0...v0.27.0) (2026-07-29)


### App Features

* **app:** add reconnect shortcut (Cmd+Shift+R) ([00c2b68](https://github.com/suiflex/rdb/commit/00c2b68565d8fa839f9c810ce8a647a323e11e55))
* **app:** group command palette results GitHub-style ([0f3151b](https://github.com/suiflex/rdb/commit/0f3151b21c6fe28d38c07e25ea4ff27a729c7bcf))
* **app:** run selected statements into separate result tabs ([6d674b1](https://github.com/suiflex/rdb/commit/6d674b1ed2bc2b3bb7671a423b424304ba00a5a6))
* **app:** widen header search pill toward corner clusters ([fb678b9](https://github.com/suiflex/rdb/commit/fb678b97dcd07123aace63ff5ba1e5a4aeb10756))


### Bug Fixes

* **app:** hide console window on Windows ([24ccbbf](https://github.com/suiflex/rdb/commit/24ccbbfe8cb434b1deddf8883078f6a4e42253b8))
* **app:** persist result filter across tab and query switches ([1c49eee](https://github.com/suiflex/rdb/commit/1c49eeec33c60a2255a471873fcd968e0e1da7b0))
* **app:** size details value boxes to content with scroll ([b2bb088](https://github.com/suiflex/rdb/commit/b2bb088ffce68e888d7d73829a85aa863ea41f96))

## RDB: [0.26.0](https://github.com/suiflex/rdb/compare/v0.25.2...v0.26.0) (2026-07-28)


### App Features

* **app:** add run/open/insert/copy actions to saved query rows ([91c768c](https://github.com/suiflex/rdb/commit/91c768c97efc25601cd6061784447052f3a173c6))
* **app:** add save-to-queries action to history rows ([b186894](https://github.com/suiflex/rdb/commit/b1868941f9360eadede0e375deedb81c095ab399))
* **app:** persist saved queries and support delete ([76d8c34](https://github.com/suiflex/rdb/commit/76d8c34b65404ccb7d9ee4d07968cf27601c04f9))
* **app:** polish keyboard shortcuts dialog ([cecbc77](https://github.com/suiflex/rdb/commit/cecbc7729d697fc560011e314145162679d12e83))
* **app:** redesign settings into tabbed sections ([d546c6f](https://github.com/suiflex/rdb/commit/d546c6fb786604a1aff81042402f28af822d8a80))
* **app:** show actionable update prompt in settings ([9287c4a](https://github.com/suiflex/rdb/commit/9287c4aa31a727a59fd98199b78f91399b1e6a80))


### Bug Fixes

* **app:** even out keyboard shortcut key chips ([b29fa66](https://github.com/suiflex/rdb/commit/b29fa66b463049f8429d11492b2810fb0611550f))
* **app:** list all six supported engines in About ([a915b5f](https://github.com/suiflex/rdb/commit/a915b5fde0c641fc0eaa721abe1f711c8d7697ec))

## RDB: [0.25.2](https://github.com/suiflex/rdb/compare/v0.25.1...v0.25.2) (2026-07-28)


### Bug Fixes

* **app:** improve editor history and result tabs ([#146](https://github.com/suiflex/rdb/issues/146)) ([fe181e5](https://github.com/suiflex/rdb/commit/fe181e5489d377d3f83d6d87d6277b4e9ec5c36d))

## RDB: [0.25.1](https://github.com/suiflex/rdb/compare/v0.25.0...v0.25.1) (2026-07-28)


### Bug Fixes

* **app:** connection-switch data loss, result-tab state, autocomplete, editor, JSON ([#143](https://github.com/suiflex/rdb/issues/143)) ([88e4653](https://github.com/suiflex/rdb/commit/88e4653020d93efb07bf89e1988f9273df724009))

## RDB: [0.25.0](https://github.com/suiflex/rdb/compare/v0.24.0...v0.25.0) (2026-07-27)


### App Features

* **app:** sidebar Queries/History split and Details panel actions ([#141](https://github.com/suiflex/rdb/issues/141)) ([ad764b5](https://github.com/suiflex/rdb/commit/ad764b5c888f28f880d7df478c4436d2e31ccb03))


### Bug Fixes

* **app:** query-pane limit, autocomplete, and result-grid UX ([#139](https://github.com/suiflex/rdb/issues/139)) ([154b123](https://github.com/suiflex/rdb/commit/154b123c5acdf1070d71daa6c03a19a7adc1116f))

## RDB: [0.24.0](https://github.com/suiflex/rdb/compare/v0.23.0...v0.24.0) (2026-07-26)


### App Features

* **app:** add New Connection action and drawn social badges to picker ([0b612f9](https://github.com/suiflex/rdb/commit/0b612f9e8bb1f140d0bb614c497889e26c3963b5))
* **app:** add Product Hunt and GitHub badges to the connection picker ([04e97cd](https://github.com/suiflex/rdb/commit/04e97cde44ead3bea6478ea39ab3bb3101a2bedd))
* **app:** add row separators and copy inspector to Mongo tree ([9a06aa4](https://github.com/suiflex/rdb/commit/9a06aa44a80da2d73c0673e5fab1aa3a316dde5e))
* **app:** connection UX, Mongo tree, SQLite fixes, and social badges ([350b7cd](https://github.com/suiflex/rdb/commit/350b7cd4268d165720cb4bca5ea311cb66e0b5ca))
* **app:** show connection status and reconnect in the header ([700927e](https://github.com/suiflex/rdb/commit/700927efb9dc6800ef4eb9016c5119795d5e728d))


### Bug Fixes

* **app:** abort in-flight connect when switching connections ([4eccfa1](https://github.com/suiflex/rdb/commit/4eccfa1cc9d4177ac43e94da65c9ab0ea1b5769c))
* **app:** center the P mark in the Product Hunt badge ([2b72f55](https://github.com/suiflex/rdb/commit/2b72f5555a83d3330400374a02fdb63c9b51300d))
* **app:** drop the P sub-box that would not center in the PH badge ([722646d](https://github.com/suiflex/rdb/commit/722646d658d8046570967bc431f513dc8f1b7856))
* **app:** skip host/port validation for SQLite connections ([af3965a](https://github.com/suiflex/rdb/commit/af3965afe684b97e907ed4297a2ec277d3c314ae))
* **app:** stop long Mongo tree values from overflowing rows ([4a0075d](https://github.com/suiflex/rdb/commit/4a0075d8636b1f254a714f7303a42910ab0c7ee8))
* **app:** use ASCII plus in the New Connection button ([a9e13e5](https://github.com/suiflex/rdb/commit/a9e13e51a59990ead1af98466d24fccc77988f2c))

## RDB: [0.23.0](https://github.com/suiflex/rdb/compare/v0.22.0...v0.23.0) (2026-07-24)


### Features

* populate primary/foreign key flags in the drivers ([5a0ab87](https://github.com/suiflex/rdb/commit/5a0ab87bcbfabd909c709aac898b2dea9c81181a))
* **ui:** add a footer Reconnect button after a dropped connection ([9184ae9](https://github.com/suiflex/rdb/commit/9184ae982c0b13f7afa4c5fb9fb15abf820bc8c7))
* **ui:** mark PK/FK columns in the sidebar field tree ([6808d91](https://github.com/suiflex/rdb/commit/6808d91ab5d99bf95a546775ec0e4021b1f2e7af))
* **ui:** open a centered inspector card for read-only cells ([5db8cd1](https://github.com/suiflex/rdb/commit/5db8cd1f41358a03bd7db3fdd0abd654457c62d3))
* **ui:** show a loading overlay while the sidebar tree reloads ([4a9cb6a](https://github.com/suiflex/rdb/commit/4a9cb6a25464f5157ca8605e326a291ed973a1d3))


### Bug Fixes

* **editor:** skip folded lines when moving the cursor vertically ([801659e](https://github.com/suiflex/rdb/commit/801659eccc2e4fda73f32261000535f38218e264))
* **ui:** add a visible vertical scrollbar to the results grid ([b552fb2](https://github.com/suiflex/rdb/commit/b552fb296a95e2011f441f25cbb5c39c8b074dff))
* **ui:** dismiss column popup when the browsed table changes ([a854349](https://github.com/suiflex/rdb/commit/a854349983aac522e987f41d33ebefb27c68375b))
* **ui:** expand an SQL table on single click, open on double click ([f321d6e](https://github.com/suiflex/rdb/commit/f321d6efa30f2c80096159413f95d5da8829d79a))
* **ui:** pan grid columns manually so the vertical wheel scrolls rows ([da04d6c](https://github.com/suiflex/rdb/commit/da04d6c03b4e45b4c5393d8e0327366f8c2e9e0f))
* **ui:** pan the grid horizontally via a wide container ([7936802](https://github.com/suiflex/rdb/commit/79368020b87d1e40af1d530e06dc0cca72975fe6))
* **ui:** scroll rows on a vertical wheel instead of panning columns ([0edb03a](https://github.com/suiflex/rdb/commit/0edb03ab3984632b220263be8d421daf530f06a3))
* **ui:** scroll the grid both ways via one two-axis ScrollView ([4f86442](https://github.com/suiflex/rdb/commit/4f86442141e8080bb39e826fda28c0c15d86a2f0))
* **ui:** scroll the grid both ways with nested Flickables ([211616b](https://github.com/suiflex/rdb/commit/211616be6846ce4bc2e4393f487f19d4dc041564))
* **ui:** show expand caret and align Mongo/Redis db rows ([1985605](https://github.com/suiflex/rdb/commit/1985605c3cd74ac91de302363e9bbbd39e1a89bc))
* **ui:** stop a table double-click collapsing into single clicks ([b5fa8cb](https://github.com/suiflex/rdb/commit/b5fa8cbb2620f6d4fad48d62cf27f402a9ad95ef))
* **ui:** use a neutral border for the read-only cell inspector ([c00bb49](https://github.com/suiflex/rdb/commit/c00bb494d6d67a215ada0c85f929742f9c96e4fc))


### Performance Improvements

* **ui:** custom-virtualize the grid for smooth two-axis scrolling ([ca517f6](https://github.com/suiflex/rdb/commit/ca517f6408f9c7a89387ceeb3a82880d81a569f9))
* **ui:** virtualize grid rows again for smooth scrolling ([fb0a124](https://github.com/suiflex/rdb/commit/fb0a12412313a4258c03e60142f225c3e1984986))

## RDB: [0.22.0](https://github.com/suiflex/rdb/compare/v0.21.0...v0.22.0) (2026-07-23)


### Features

* create db/schema/table, live connection status, update nudges ([0e2b826](https://github.com/suiflex/rdb/commit/0e2b826ed315afeb1d023720c1b73ffbc60caa0e))
* desktop notification when an update is available ([1bbe1a4](https://github.com/suiflex/rdb/commit/1bbe1a4c8930fc1c9a3631c5af56a86bc5f02a6f))
* live connection status dot with periodic ping ([f8f2550](https://github.com/suiflex/rdb/commit/f8f255062448001970763cf0f912a8f48357bd00))
* manual "check for updates" in settings ([b326308](https://github.com/suiflex/rdb/commit/b326308f7dda3021e3fa0b6638a81dc30bee6d62))
* table designer dialog from sidebar + ([3b1cd1c](https://github.com/suiflex/rdb/commit/3b1cd1c91f04e34a2ce3b1ca9ca170db371f6f88))


### Bug Fixes

* create database/schema from modal New… instead of connection form ([3a2ce95](https://github.com/suiflex/rdb/commit/3a2ce9546a1edef4c3ca64f09784989b6e508b7a))
* reflect live connection health in the status footer ([d4489f8](https://github.com/suiflex/rdb/commit/d4489f81decedc7197f90ca4bee2a7016efbb873))
* SQL editor UX batch — cancel, autocomplete, folding, cell copy ([#127](https://github.com/suiflex/rdb/issues/127)) ([8d4d51e](https://github.com/suiflex/rdb/commit/8d4d51eccbec519fcfce050edf8cd0fbbb6a5201))

## RDB: [0.21.0](https://github.com/suiflex/rdb/compare/v0.20.0...v0.21.0) (2026-07-22)


### Features

* **export:** embed real password in exported connection URLs ([5a10ed1](https://github.com/suiflex/rdb/commit/5a10ed165e75f20393d531bdc40ac3ef96ceb986))
* **ui:** add copy/export/chart to the table browse toolbar ([7d33e1a](https://github.com/suiflex/rdb/commit/7d33e1aec39bab5e2ba091ff3a543cde21811ff0))
* **ui:** show a spinner while the tree reloads on schema switch ([1af6362](https://github.com/suiflex/rdb/commit/1af636257c1760352a182a7f0b7ba74be4d671e5))


### Bug Fixes

* **export:** drop unused non-macos activation-policy stub ([00ef039](https://github.com/suiflex/rdb/commit/00ef039608dfb6e24ae45c55e166fa43dbf72372))
* **export:** emit the chosen format before closing the menu ([3e96d50](https://github.com/suiflex/rdb/commit/3e96d504927218a64a0854d3c6cc8f133f1ec833))
* **export:** force Regular activation policy at dialog open, not just startup ([53d11f6](https://github.com/suiflex/rdb/commit/53d11f67323c870ec88dcd48f036664280375725))
* **export:** make save dialog appear reliably on macOS ([d6225ba](https://github.com/suiflex/rdb/commit/d6225ba99c3cd2d9445cb8759a7092e1f60eff8b))
* **export:** parent the save panel so rfd uses its async sheet path ([511329f](https://github.com/suiflex/rdb/commit/511329fbdb6f584b673a7af6748762593c5cd4c9))
* **export:** set macOS activation policy to Regular so save dialog shows ([cb962de](https://github.com/suiflex/rdb/commit/cb962de2be5aa59b051cf336d57913b532aa6326))
* **export:** stop the format menu closing before its click registers ([13a12f4](https://github.com/suiflex/rdb/commit/13a12f4c11df78b14b6519df628b411ebd0004dc))
* **export:** use osascript save panel on macOS ([550fdea](https://github.com/suiflex/rdb/commit/550fdeaf97c3a707230c2f396ebf1137559d7ee5))
* macOS export dialog, connection secrets, tab persistence + UI polish ([0b41fa3](https://github.com/suiflex/rdb/commit/0b41fa34a93b45db2392afb27b6e56c4f6decd39))
* **ui:** keep SQL query tabs and results across a connection switch ([cd75c1e](https://github.com/suiflex/rdb/commit/cd75c1ee2d5ae10a25a16d83520fdc2bde70722a))
* **ui:** show that a saved connection already has a password ([095d8f0](https://github.com/suiflex/rdb/commit/095d8f0ca18dfde1db5382360c8fe8932c292e9c))
* **ui:** stop query-tab clicks misfiring as drags ([0cc96d6](https://github.com/suiflex/rdb/commit/0cc96d63bece5b2cc64beab044f399d70940d70e))

## RDB: [0.20.0](https://github.com/suiflex/rdb/compare/v0.19.0...v0.20.0) (2026-07-21)


### Features

* **ui:** use grip icon for connection drag handle ([5837ec9](https://github.com/suiflex/rdb/commit/5837ec9a8064e931d8648eb4e45c999ae076b65f))


### Bug Fixes

* **app:** show connections save dialog and add URL to export ([b5eaf4e](https://github.com/suiflex/rdb/commit/b5eaf4e0bbcbceef10184b6228802a8707cb0877))
* **app:** surface export save-dialog scheduling failure ([3d56ffa](https://github.com/suiflex/rdb/commit/3d56ffa315533b1bd423a2a2cb7ec9153ec4def0))
* clear RUSTSEC advisories [#98](https://github.com/suiflex/rdb/issues/98)–[#106](https://github.com/suiflex/rdb/issues/106) ([#110](https://github.com/suiflex/rdb/issues/110)) ([a991b72](https://github.com/suiflex/rdb/commit/a991b72e727fa20f82e75e5df740aa564ec75bae))
* export save dialog, connection URL, and tab/drag icons ([5a75f56](https://github.com/suiflex/rdb/commit/5a75f56bfa77371f9437c3f963572a79aaeb54e0))
* **ui:** replace tab move glyph with columns icon ([32f301d](https://github.com/suiflex/rdb/commit/32f301df2ab5419f0b98ca11036e7dbf0fe48adc))
* **ui:** use columns icon for right-pane tab move button ([4830bb5](https://github.com/suiflex/rdb/commit/4830bb5078fdb1712acbc5854908c6e00f9660ea))

## RDB: [0.19.0](https://github.com/suiflex/rdb/compare/v0.18.0...v0.19.0) (2026-07-20)


### Features

* **app:** accent bar on the focused split pane ([982374d](https://github.com/suiflex/rdb/commit/982374d0135313eccc4de82ac7e37bcbac72320a))
* **app:** add split toggle rendering the second query pane ([325bebf](https://github.com/suiflex/rdb/commit/325bebf30b4020be8ab0e0a1a827646e416aeb0f))
* **app:** complete split pane execution controls ([97a59d6](https://github.com/suiflex/rdb/commit/97a59d6eb9057e90e933e8f1c48227b79d016c16))
* **app:** drag tabs between workspace groups ([63854c5](https://github.com/suiflex/rdb/commit/63854c5ef69162f40a61623477b8b906e9fea8c2))
* **app:** drag-reorder connections within a group in picker ([b6a9c93](https://github.com/suiflex/rdb/commit/b6a9c938d626c339d67d3b9f3fb30f5656e5be5c))
* **app:** localize table chrome by group ([c936fa4](https://github.com/suiflex/rdb/commit/c936fa451ae88f8af40b312a519e354f08c32d71))
* **app:** per-pane find (Cmd+F targets the focused pane) ([2a0ef4c](https://github.com/suiflex/rdb/commit/2a0ef4c2180e994af2d4d629e5d6bd9babcca04a))
* **app:** persist focused group and each group's active tab ([54bb1cd](https://github.com/suiflex/rdb/commit/54bb1cd0706b09e46ce15d83fbb18bb2d03e4d87))
* **app:** persist split pane layout ([c9ecfe1](https://github.com/suiflex/rdb/commit/c9ecfe105205008866ddc0f244917ee11afdc3fe))
* **app:** remember split and right-pane text per tab ([ca8f5c2](https://github.com/suiflex/rdb/commit/ca8f5c2ea63b69fd945558680b7112ebd0c19916))
* **app:** run queries in the right split pane (buffered) ([6b7c557](https://github.com/suiflex/rdb/commit/6b7c5576aca3cdf2faec9735034289b7fefecab2))
* **app:** sort connections by favorite then order in picker ([05a8b4e](https://github.com/suiflex/rdb/commit/05a8b4e62b4080a12fc7a3b3ece04ce50f74361c))
* **app:** star and drag-reorder saved connections ([211ee44](https://github.com/suiflex/rdb/commit/211ee4482351761101c3ba1fe9fd6a02cfb97b4c))
* **app:** star badge and toggle in connection picker ([b06009d](https://github.com/suiflex/rdb/commit/b06009d4a78386d38ec9528376f77cf85a462805))
* **app:** wire per-pane editor input (keys, mouse, fold) ([5613cec](https://github.com/suiflex/rdb/commit/5613cec50ac198aa92f70e619a49d13ad8a35c41))
* **app:** workspace tab groups (split editor with independent groups) ([b19983e](https://github.com/suiflex/rdb/commit/b19983e908dede53b134104959c35b5677ed0809))
* **ui:** add pane 1 mirror properties and bind second pane ([89b7942](https://github.com/suiflex/rdb/commit/89b79429a11f8fb420a288d8f21165bdad1f601a))
* **ui:** wrap query pane in split layout with empty second pane ([442718d](https://github.com/suiflex/rdb/commit/442718d44f008185efa7b8cf9b7f6440e26f1409))


### Bug Fixes

* **app:** attach parent window to export save dialog ([98268b0](https://github.com/suiflex/rdb/commit/98268b09bbf8fb37e9b3813256f9db76f8ebc178))
* **app:** clone streaming group state ([e4614b6](https://github.com/suiflex/rdb/commit/e4614b6e4cda60911ab6eff62f36b5ce26744140))
* **app:** enable autocomplete in split pane ([8d4e62b](https://github.com/suiflex/rdb/commit/8d4e62bb8422fd73fdd56adc52ef69c5a352b8a9))
* **app:** finalize workspace tab groups ([9af02e8](https://github.com/suiflex/rdb/commit/9af02e8754037ad6278a2779f21f99b3169c4acd))
* **app:** improve workspace tab controls ([87ba2a4](https://github.com/suiflex/rdb/commit/87ba2a4381930019d1103f1b2add69dc9d2dbe79))
* **app:** keep picker star clickable and add drag affordance ([cad3cae](https://github.com/suiflex/rdb/commit/cad3cae79ada09068e134c69212e735909399489))
* **app:** keep query tabs and results when switching connection ([b8c8d7e](https://github.com/suiflex/rdb/commit/b8c8d7eab3e01f1673e69aabb9aa32eb8efc4bd1))
* **app:** make result grid vertical scroll reliable ([7a56c1a](https://github.com/suiflex/rdb/commit/7a56c1a3b238ebc74b4e251718205cef4ee58f73))
* **app:** persist right-pane result per tab ([a0db1b8](https://github.com/suiflex/rdb/commit/a0db1b8a03f60f8e7f353597bb3e8c2625da1689))
* **app:** reliable grid scroll, export dialog, and standby tabs ([cfb5289](https://github.com/suiflex/rdb/commit/cfb5289368474aa13925cef618175a6add9ddf13))
* **app:** reset split so a new tab starts unsplit ([18153e7](https://github.com/suiflex/rdb/commit/18153e72e22e5a4447100a810fbae6aa6d784c73))
* **app:** route Cmd+F/Cmd+Enter to focused pane, add Cmd+D split ([25f52d5](https://github.com/suiflex/rdb/commit/25f52d5d343f9eaa3168d72c907835fea5517218))
* **app:** route workspace shortcuts by group ([4fb2707](https://github.com/suiflex/rdb/commit/4fb2707caa00459936b640ffbf504d9290942c5b))
* **app:** run the focused pane on Cmd+Enter, not always the left ([b138b19](https://github.com/suiflex/rdb/commit/b138b19c9cd2dae1323dc5290d17e182cd24fb99))
* **app:** satisfy clippy on restore_p1_tab and drop redundant binding ([e07a689](https://github.com/suiflex/rdb/commit/e07a689b9ac11bd4ba09c1e447f74d73e926f61a))
* **app:** stabilize grouped tab state ([5a46add](https://github.com/suiflex/rdb/commit/5a46addf33df59c85fc28444db5f55d6d1a5b157))
* **app:** wire split pane grid interactions ([50134f8](https://github.com/suiflex/rdb/commit/50134f8ea8a8fa2ad45852d0861fb00e90388ca2))
* **ui:** remove legacy pane split controls ([f349a44](https://github.com/suiflex/rdb/commit/f349a44ccf77786df79087b7b065320efef24921))

## RDB: [0.18.0](https://github.com/suiflex/rdb/compare/v0.17.0...v0.18.0) (2026-07-18)


### Features

* **app:** add connections CSV serializer ([9d06f5d](https://github.com/suiflex/rdb/commit/9d06f5dd8a42a8c9f8ff625cf04bc4d3c46fb9db))
* **app:** allow the sidebar on the right side ([884b6db](https://github.com/suiflex/rdb/commit/884b6db44036439d00a3878ab3f05a8e5d3846c5))
* **app:** export connections as JSON or CSV ([8d2e116](https://github.com/suiflex/rdb/commit/8d2e116fdadbd090b91b26d53a8ffd9d8a264416))
* **app:** export connections as JSON or CSV ([a3386b1](https://github.com/suiflex/rdb/commit/a3386b19430d9c8bfdf778e813a67da79aa8d4f0))
* **app:** export query results in multiple formats ([e025f8d](https://github.com/suiflex/rdb/commit/e025f8da8433f44c985d1b2f41308a6a622844f4))
* **app:** server-side column filter (WHERE) for table browse ([2d9437f](https://github.com/suiflex/rdb/commit/2d9437fa5194b97b79100489d8998f98d7f54e4f))
* movable sidebar (left/right) with settings toggle ([bed919b](https://github.com/suiflex/rdb/commit/bed919b964083c93745b5deda5c09b668bbdedb8))
* SQL console UX — results toggle, export formats, filter ([4018ac7](https://github.com/suiflex/rdb/commit/4018ac78437cb6af05bdd5f58bcfe4afdd10a1ac))
* **ui:** collapse sidebar to icon rail on drag-to-min ([264d576](https://github.com/suiflex/rdb/commit/264d5760685345f0be8dc0944dd4aea223b186c0))
* **ui:** results toggle, tooltip, export menu, per-column filter ([e9fdbe9](https://github.com/suiflex/rdb/commit/e9fdbe9e80a65f8a2b3a8b9103ddae5717038648))
* **ui:** workarea + sidebar UX fixes and server-side table filter ([61e01a5](https://github.com/suiflex/rdb/commit/61e01a54a3a7081dacfb7c6a76cb1eb7c69f4b03))


### Bug Fixes

* **app:** collapse sidebar to icon rail from the header toggle ([40edf1a](https://github.com/suiflex/rdb/commit/40edf1a93dce47c8c72686060cbaf01abb7dda54))
* **app:** keep result grid visible when switching schema ([1b7d98b](https://github.com/suiflex/rdb/commit/1b7d98ba2f6d8c87eb6ffad16d9b5217f1a9515a))
* **app:** keep SQL query tabs when switching schema ([5982220](https://github.com/suiflex/rdb/commit/59822201c19fa55d7db3aafe9f08306774a9da34))
* **app:** mirror the sidebar header toggle icon on the right ([219c180](https://github.com/suiflex/rdb/commit/219c18028f818f1ad5821ec8c20c2fefedfad214))
* **app:** place the sidebar toggle button on the sidebar's side ([a8c174e](https://github.com/suiflex/rdb/commit/a8c174ee9976f2764dbf34faf95b48eba2e6c2a5))
* **dispatch:** drop stale allow(dead_code) ([e6b03ae](https://github.com/suiflex/rdb/commit/e6b03ae92163e681ba5b1a0a50feba6fca44748f))
* **ui:** center Export button in results toolbar ([0381013](https://github.com/suiflex/rdb/commit/0381013306c140100ab59aa5ba6b0cbd382c553e))
* **ui:** collapse sidebar to icon rail, not reveal sliver ([43be444](https://github.com/suiflex/rdb/commit/43be444b2e45b70b6551a362a1086e315e13be1d))
* **ui:** latch sidebar rail to stop drag flicker ([98c8fa7](https://github.com/suiflex/rdb/commit/98c8fa757f463dac3e0d0bd807df3b16bd1500a7))
* **ui:** show Export menu by removing popup height cycle ([1e8d1e3](https://github.com/suiflex/rdb/commit/1e8d1e33070b6f4b3a184aea00c4d1d35d0b9e3a))

## RDB: [0.17.0](https://github.com/suiflex/rdb/compare/v0.16.0...v0.17.0) (2026-07-16)


### Features

* **header:** show theme toggle on connections screen too ([d952ac0](https://github.com/suiflex/rdb/commit/d952ac0ec17c5f99955169de99a57dc9912b56be))
* **picker:** add engine brand glyph assets + icon mappings ([4b8506c](https://github.com/suiflex/rdb/commit/4b8506cfb3d29ad92e9676f7fbbf55c72a839a01))
* **picker:** engine-tinted connection cards, brand logos, header theme toggle ([5cd1a9d](https://github.com/suiflex/rdb/commit/5cd1a9dcc447b552baf0c6183900a59f29da18e3))
* **picker:** show engine brand logo on connection badge ([e3c2269](https://github.com/suiflex/rdb/commit/e3c22690e25cd5a6a7ce8d3e974ab61aaf8b13a4))
* **picker:** show RDB logo on empty connection panel ([d0ad469](https://github.com/suiflex/rdb/commit/d0ad4690aeff0d1a9c9b8b331a2a48c7936d430d))
* **picker:** tint connection row card per engine color ([c8c523c](https://github.com/suiflex/rdb/commit/c8c523c52a2191350a8d9b9bb7af064d726e0784))
* **ui:** add Tooltip global + shared overlay layer ([ccbe616](https://github.com/suiflex/rdb/commit/ccbe61675d9914f31ac8ec28376461d360e842bd))

## RDB: [0.16.0](https://github.com/suiflex/rdb/compare/v0.15.0...v0.16.0) (2026-07-16)


### Features

* **grid:** stream "No limit" results progressively with cancel ([c47ff28](https://github.com/suiflex/rdb/commit/c47ff28af639965567f9b6d9e7e082ac4cf3d680))


### Bug Fixes

* **grid:** cap manual SELECT rows and virtualize the result grid ([43f06e0](https://github.com/suiflex/rdb/commit/43f06e0778f8aa426d8200d3481e841606c7977d))
* **grid:** fix SELECT * freeze — row cap, virtualized grid, streaming No-limit ([9fce5b5](https://github.com/suiflex/rdb/commit/9fce5b55f1cc5f73247550d4bf947c91071c9e60))

## RDB: [0.15.0](https://github.com/suiflex/rdb/compare/v0.14.0...v0.15.0) (2026-07-15)


### Features

* **app:** native Save-As dialog for CSV export ([1b5878e](https://github.com/suiflex/rdb/commit/1b5878e488074e1c5d501f23333ad9a2160f415d))


### Bug Fixes

* **ui:** panel toggles eaten by tooltip; native Export Save-As ([3363eb5](https://github.com/suiflex/rdb/commit/3363eb5e9d64102f555d40b8cba0c6824c42a507))
* **ui:** remove redundant footer Export buttons ([4c567a4](https://github.com/suiflex/rdb/commit/4c567a4242b858fcb5a21cfe3a6287053be6e371))
* **ui:** stop tooltips from swallowing button clicks ([6c4c819](https://github.com/suiflex/rdb/commit/6c4c819b05b0bcf01feeee87e25d0bce06b908e4))

## RDB: [0.14.0](https://github.com/suiflex/rdb/compare/v0.13.0...v0.14.0) (2026-07-15)


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

## RDB: [0.13.0](https://github.com/suiflex/rdb/compare/v0.12.0...v0.13.0) (2026-07-14)


### Features

* **ui:** add TablePlus-style workspace tabs ([55987a1](https://github.com/suiflex/rdb/commit/55987a11305b4e0b2ba419b848e45e779954911b))
* **ui:** add TablePlus-style workspace tabs ([7c98c15](https://github.com/suiflex/rdb/commit/7c98c15e2763991200876c1c99128bf9badfb582))

## RDB: [0.12.0](https://github.com/suiflex/rdb/compare/v0.11.0...v0.12.0) (2026-07-13)


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

## RDB: [0.11.0](https://github.com/suiflex/rdb/compare/v0.10.0...v0.11.0) (2026-07-12)


### Features

* **app:** add Options/URI field to the connection form ([e5d9b34](https://github.com/suiflex/rdb/commit/e5d9b34d7091d532ae3648982302d0d68f967093))
* **brand:** align RDB logo + README with suiflex org ([a88d56d](https://github.com/suiflex/rdb/commit/a88d56dfe84bc110976eec9e662ab6f0f98d5dc4))


### Bug Fixes

* **editor:** full SQL autocomplete with clean identifier insert ([e054a84](https://github.com/suiflex/rdb/commit/e054a84568c10ffbb5988d5822bf7bfb57d38637))
* mongo connect, SQL autocomplete, and editor toolbar UX ([b984bf0](https://github.com/suiflex/rdb/commit/b984bf0ebfd635f52d54229dd2b94f0a1b877298))
* **ui:** label the disconnect button with a hover tooltip ([95e73f3](https://github.com/suiflex/rdb/commit/95e73f344db599ed5bf7663b90b4013346f7497d))
* **ui:** right-align the Limit stepper ([2f6a693](https://github.com/suiflex/rdb/commit/2f6a693d4b8f98f62734358d086961312d84b140))

## RDB: [0.10.0](https://github.com/suiflex/rdb/compare/v0.9.0...v0.10.0) (2026-07-11)


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

## RDB: [0.9.0](https://github.com/suiflex/rdb/compare/v0.8.0...v0.9.0) (2026-07-10)


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

## RDB: [0.8.0](https://github.com/suiflex/rdb/compare/v0.7.0...v0.8.0) (2026-07-09)


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

## RDB: [0.7.0](https://github.com/suiflex/rdb/compare/v0.6.0...v0.7.0) (2026-07-09)


### Features

* **app:** add selected-cell inspector for full and JSON values ([0f1d3f8](https://github.com/suiflex/rdb/commit/0f1d3f8f7360427194f6942b275aa450699772af))
* **app:** make cell inspector resizable by dragging its top edge ([d01bd1d](https://github.com/suiflex/rdb/commit/d01bd1d306497e386552c916ef506d8abc98bc87))


### Bug Fixes

* **app:** bubble editor keys to window shortcut scope for run ([eadb051](https://github.com/suiflex/rdb/commit/eadb051be09826c0c4f16d18cbc7ea324293c154))
* **app:** reset last tab on close and clear grid on new tab ([a111ce7](https://github.com/suiflex/rdb/commit/a111ce7290460b130c40be66acb6ce86959ca37d))
* **app:** search icon, ⌘⏎ run, Postgres JSON cells, tab close/new ([4e91b9a](https://github.com/suiflex/rdb/commit/4e91b9aa8be4afc32beb27d3ef3e4343d491f7a2))
* **app:** use search SVG icon in search and filter fields ([ffa30d4](https://github.com/suiflex/rdb/commit/ffa30d45de70ad5448f474a261f5aaf9618471e9))

## RDB: [0.6.0](https://github.com/suiflex/rdb/compare/v0.5.0...v0.6.0) (2026-07-08)


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

## RDB: [0.5.0](https://github.com/suiflex/rdb/compare/v0.4.0...v0.5.0) (2026-07-04)


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

## RDB: [0.4.0](https://github.com/suiflex/rdb/compare/v0.3.0...v0.4.0) (2026-07-03)


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

## RDB: [0.3.0](https://github.com/suiflex/rdb/compare/v0.2.0...v0.3.0) (2026-07-01)


### Features

* **mongo:** bound collection browse with a row limit ([e1de976](https://github.com/suiflex/rdb/commit/e1de976cc32fd4d7ea7143a3071876056d9a512d))


### Bug Fixes

* **mongo:** correct sidebar tree and data preview ([14a3e5e](https://github.com/suiflex/rdb/commit/14a3e5e15e30800d9c4b97fa6e04e56dc3655343))
* **ui:** show loading/empty placeholder for mongo databases ([d390b24](https://github.com/suiflex/rdb/commit/d390b248eb176c4c1de3faf535d8e98f5ca8978d))

## RDB: [0.2.0](https://github.com/suiflex/rdb/compare/v0.1.0...v0.2.0) (2026-06-22)


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
