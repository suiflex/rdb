# Changelog

## [0.37.0](https://github.com/suiflex/rdb/compare/v0.36.0...v0.37.0) (2026-08-13)


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
