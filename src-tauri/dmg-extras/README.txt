TermFast 安装说明
=================

1. 将左侧 TermFast 拖到右侧 Applications 文件夹

2. 首次打开如提示"已损坏"或"无法验证开发者"：
   双击 fix-quarantine.command
   或在终端执行：
   xattr -cr /Applications/TermFast.app

3. 之后即可正常打开 TermFast

（此提示是因为 app 尚未做 Apple Developer ID 签名，后续版本会解决）
