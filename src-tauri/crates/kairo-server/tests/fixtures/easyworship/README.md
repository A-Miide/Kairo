# EasyWorship files for the import tests

Real files saved by EasyWorship, used by `easyworship.rs`'s `real_easyworship_files_import` test.
The songs in them are public domain hymns and placeholder text.

| Here | Format | From |
|---|---|---|
| `ew6/Songs.db`, `ew6/SongWords.db` | EasyWorship 6/7 song library | OpenLP, `tests/resources/songs/easyworship/ew6/Databases/Data/` |
| `ew2009/Songs.DB`, `ew2009/Songs.MB` | EasyWorship 2007/2009 song library | OpenLP, `tests/resources/songs/easyworship/` |
| `test1.ewsx` | EasyWorship 6/7 schedule | OpenLP, `tests/resources/songs/easyworship/` |
| `test1.ews` | EasyWorship 2009 schedule | OpenLP, `tests/resources/songs/easyworship/` |
| `ews-lithium/file1.ews`, `ews-lithium/lorem.ews` | EasyWorship 2009 schedule | lithium-ews, `src/test/resources/lithium/io/ews/` |

Sources, both under the GNU GPL version 3, as Kairo is:
- OpenLP: https://gitlab.com/openlp/openlp (Copyright (c) OpenLP Developers)
- lithium-ews: https://github.com/meinders/lithium-ews (Copyright (c) Gerrit Meinders)

The files are unchanged. Each library's two files sit in a folder of their own, as the test
expects.
