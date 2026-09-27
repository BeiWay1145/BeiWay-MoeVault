# 协作约定（AGENTS.md）

本文件是项目的常驻协作约定，每个会话都应遵守。

## 构建

**每次做出修改后，都要构建 MSI 打包产物并安装验证**（用于人工测试）：
```
cd src-tauri; cargo tauri build
```
产物：`src-tauri/target/release/bundle/msi/BeiWay-MoeVault_0.1.0_x64_en-US.msi`

## 提交与推送

**每次进行修改的会话结束后都要 `git commit` 并 `git push`**。

- 会话结束时（本轮所有改动都已落地、构建验证完成）即提交一次，不需要等待用户额外确认
- 提交后**立即推送到远端**（`git push`）
- 提交前：跑通 `cargo test --workspace` 与 `npm run build`
- 提交信息用中文、写清本轮改动范围

## 测试卫生

- 只删除**能证明是自己创建的**测试文件（按内容 MD5 或唯一文件名匹配）
- 不使用按 id 区间盲目删除数据
- 测试脚本放在 `scripts/`（已被 .gitignore 排除，本地保留）
