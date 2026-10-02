# Changelog

## [0.48.2](https://github.com/suiflex/rdb/compare/v0.48.1...v0.48.2) (2026-10-02)


### Bug Fixes

* **app:** drop a sidebar expand that lands on another connection ([2793b5c](https://github.com/suiflex/rdb/commit/2793b5cbdf766eb53b874c00e94b2ce0bfe2a808))
* **app:** filter the database and schema switchers as you type ([2ecc516](https://github.com/suiflex/rdb/commit/2ecc5163296d389ecd6a0ea770f105c5afef2a83))
* **app:** filter the Queries and History sidebar lists ([4c27d0b](https://github.com/suiflex/rdb/commit/4c27d0b64b48caaeb0903bea8a6f6693f3839471))
* **app:** keep one in-flight connect per connection ([4754702](https://github.com/suiflex/rdb/commit/47547026f6fdb1e7299ec97cb57e849372dd17f6))
* **app:** keep rail selection on the live connection for offline tabs ([0c6a09a](https://github.com/suiflex/rdb/commit/0c6a09a5ef0e3a2b6346e96d3528c40c249ba784))
* **app:** label restored tabs from the connection they are bound to ([19643f0](https://github.com/suiflex/rdb/commit/19643f02f3b50e445fbfdafaed0bed49d3540c99))
* **app:** let the connections rail close every connection at once ([9af5b3a](https://github.com/suiflex/rdb/commit/9af5b3a5cd42a301accb6d49135dcf88efca7191))
* **app:** match searches the way query completion does ([bf92566](https://github.com/suiflex/rdb/commit/bf92566cab5e6c6af90ff732c730104c5048708e))
* **app:** move a run on a dead connection's tab to the live connection ([52f87aa](https://github.com/suiflex/rdb/commit/52f87aab00bc8024b66130fd0044339545f50b09))
* **app:** move to a live connection when the focused one disconnects ([32cbc97](https://github.com/suiflex/rdb/commit/32cbc97c37ca61cada1a6910e409e3da96f8a2b0))
* **app:** never queue a connect behind another connection's connect ([d6bd512](https://github.com/suiflex/rdb/commit/d6bd512828f36df4f8691bc1d8e2e0a2d9adbbc2))
* **app:** publish a connect into the current slot only while focused ([37cb3e2](https://github.com/suiflex/rdb/commit/37cb3e227cd4691b689635887db3ff8e5770da7d))
* **app:** resolve a tab's driver without waiting on the current slot ([3a8d9f1](https://github.com/suiflex/rdb/commit/3a8d9f13e56af6afefb9d0550594e122b664e2d4))
* **app:** run a tab's query on its own database selection ([f1d6f6f](https://github.com/suiflex/rdb/commit/f1d6f6fdb6f165b03eaaa4f4fe1a60dbd91d8f01))
* **app:** show a no-match message when a search finds nothing ([c3dc983](https://github.com/suiflex/rdb/commit/c3dc9833f9ec7dc20e911b3054e1943f054eca2f))
* **app:** show an offline tab instead of auto-connecting it ([058b53a](https://github.com/suiflex/rdb/commit/058b53a719cdfc5355b620fb9c5a3aa4ee357822))
* **app:** show the focused connection's status after a tab switch ([06501bc](https://github.com/suiflex/rdb/commit/06501bc3b890ba41cd9c1e9eff435b483e8bd6df))
* **app:** start a search modal with an empty search field ([bce07a5](https://github.com/suiflex/rdb/commit/bce07a570d2f54bec97c069bb98a9cd33922ec60))
* **deps:** bump polyval to 0.7.3 so ghash builds with zeroize ([bb64656](https://github.com/suiflex/rdb/commit/bb6465611aed6aed32501189b8bd2047526cb733))
