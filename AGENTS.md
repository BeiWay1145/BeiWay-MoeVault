# 协作约定（AGENTS.md）

本文件是项目的常驻协作约定，每个会话都应遵守。

## 构建

**每次做出修改后，都要构建 MSI 打包产物并安装验证**（用于人工测试）：
```
cd src-tauri; cargo tauri build
```
产物：`src-tauri/target/release/bundle/msi/BeiWay-MoeVault_0.1.0_x64_en-US.msi`

## 提交

**每完成一轮完整对话就提交一次**——即：本轮所有新增/修改功能都已交付，且**用户已确认完成测试**后，执行一次 `git commit`。

- 提交前：跑通 `cargo test --workspace` 与 `npm run build`
- 提交信息用中文、写清本轮改动范围
- **推送（`git push`）仍需用户明确要求**，不自动推送

## 测试卫生

- 只删除**能证明是自己创建的**测试文件（按内容 MD5 或唯一文件名匹配）
- 不使用按 id 区间盲目删除数据
- 测试脚本放在 `scripts/`（已被 .gitignore 排除，本地保留）
