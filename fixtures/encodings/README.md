# 编码夹具

`canonical.md` 是这份文本的 UTF-8（无 BOM）版本，也是其余文件的基准内容。
其余文件是同一份文本写成不同编码的真实字节：

| 文件 | 编码 | BOM | 行尾 |
| --- | --- | --- | --- |
| canonical.md | UTF-8 | 无 | LF |
| utf8-bom.md | UTF-8 | 有 | LF |
| utf16le-bom.md | UTF-16 LE | 有 | LF |
| utf16be-bom.md | UTF-16 BE | 有 | LF |
| gbk.md | GBK（GB18030 兼容） | 无 | LF |

这些文件由 `buffer::file_shape` 的用例核对：解码文本必须与 `canonical.md` 逐字
相同，按同一形状重编码必须逐字节回到原文件，并且都能通过打开侧的文本嗅探。
改内容要同时重写全部文件并跑那组用例，不要用编辑器单独改其中一份。

无 BOM 的 UTF-16 与 GBK/GB18030 在字节上不可区分：它的「拉丁字母 + NUL」或
CJK 字节都可能是合法的别种编码，猜错会把用户的 GBK 笔记按 UTF-16 写出去。
因此 UTF-16 只收带 BOM 的文件，无 BOM 的 UTF-16 不给夹具、也不猜。
