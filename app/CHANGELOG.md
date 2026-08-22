# Changelog

## [0.41.0](https://github.com/suiflex/rdb/compare/v0.40.0...v0.41.0) (2026-08-22)


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
