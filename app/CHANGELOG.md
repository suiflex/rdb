# Changelog

## [0.34.0](https://github.com/suiflex/rdb/compare/v0.33.0...v0.34.0) (2026-08-09)


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
