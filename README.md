# AITools

用于集中管理由 AI 接管、构建和扩展的工具。

## 当前工具

- `QTranslate/`：跨应用划词翻译和屏幕 OCR 工具，源码来自 [ahatem/QTranslate](https://github.com/ahatem/QTranslate)。

## QTranslate

源码目录：`QTranslate/`

源码构建要求 Java 17+；当前构建使用 Java 21，Kotlin 工具链会使用 Java 11 编译兼容目标。

```powershell
cd QTranslate
.\gradlew.bat build --console=plain --no-configuration-cache `
  '-Duser.language=en' '-Duser.country=US'
```

生成源码构建的便携包：

```powershell
.\gradlew.bat assembleReleaseVariants --console=plain --no-configuration-cache `
  '-PreleaseVersion=1.5.0'
```

产物位于 `QTranslate/build/release/`。本机已将便携包展开到 `QTranslate/dist/QTranslate-1.5.0/`，运行入口为其中的 `QTranslate/QTranslate.jar`。

运行数据和翻译历史保存在便携目录的 `logs/`、`plugins_data/` 等目录中，不参与 Git 提交。
