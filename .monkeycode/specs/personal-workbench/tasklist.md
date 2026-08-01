# 需求实施计划

- [ ] 1. Rust 后端基础结构与依赖
  - [ ] 1.1 在 Cargo.toml 添加依赖（rusqlite/bundled、dirs、tauri-plugin-global-shortcut、tauri-plugin-opener）
  - [ ] 1.2 创建目录结构（commands.rs、db.rs、models.rs、repo/、config.rs、process.rs、tray.rs、shortcut.rs）
  - [ ] 1.3 配置 tauri.conf.json 无边框窗口与 capabilities 权限

- [ ] 2. SQLite 数据层
  - [ ] 2.1 实现 models.rs 数据结构（Group/Resource/Note 与排序字段）
  - [ ] 2.2 实现 db.rs 初始化与建表迁移（groups/resources/notes 三表 + 索引）
  - [ ] 2.3 实现 repo/group.rs 分组 CRUD 与排序
  - [ ] 2.4 实现 repo/resource.rs 资源 CRUD、排序、跨组移动
  - [ ] 2.5 实现 repo/note.rs 笔记 CRUD 与标题/内容检索
  - [ ]* 2.6 编写数据层单元测试（CRUD、排序、跨组移动）

- [ ] 3. 配置与窗口基础能力（需求 AC 1.1-1.11）
  - [ ] 3.1 实现 config.rs JSON 配置读写（主题、窗口状态、置顶）与原子写
  - [ ] 3.2 实现 tray.rs 系统托盘（单击切换窗口、右键菜单、退出）
  - [ ] 3.3 实现 shortcut.rs 全局快捷键 Ctrl+Shift+Space 注册
  - [ ] 3.4 在 lib.rs 实现窗口关闭拦截（隐藏至托盘）、位置尺寸记忆、置顶开关

- [ ] 4. 进程与浏览器启动（需求 AC 2.1-2.4）
  - [ ] 4.1 实现 process.rs 本地程序启动（路径、参数、错误处理）
  - [ ] 4.2 实现网页书签默认浏览器打开
  - [ ] 4.3 实现 pick_program_file 文件选择器命令

- [ ] 5. invoke 命令层整合
  - [ ] 5.1 实现 commands.rs 全部命令并注册（get_initial_data、各 CRUD、reorder、launch、search_all、config、window）

- [ ] 6. 检查点 - 后端编译通过
  - [ ] 确保 cargo build 无错误,如有疑问请询问用户

- [ ] 7. 前端基础框架
  - [ ] 7.1 创建布局（无边框标题栏、侧边导航、内容区）
  - [ ] 7.2 封装 Tauri API 调用层（window 控制、config、launch）
  - [ ] 7.3 实现主题切换与持久化

- [ ] 8. 前端快捷启动工作台（需求 AC 2.5-2.9）
  - [ ] 8.1 实现资源分组展示与分组管理（增删改）
  - [ ] 8.2 实现资源卡片（本地程序/网页书签）与新增/编辑表单
  - [ ] 8.3 实现拖拽排序（组内与跨分组）
  - [ ] 8.4 实现资源右键操作菜单与删除
  - [ ] 8.5 实现资源点击启动（invoke launch）

- [ ] 9. 前端速记笔记（需求 AC 3.1-3.6）
  - [ ] 9.1 实现笔记列表（标题、创建时间、内容摘要）
  - [ ] 9.2 实现笔记编辑区（新建、编辑、保存、删除、切换）

- [ ] 10. 前端全局搜索（需求 AC 2.9、3.7）
  - [ ] 10.1 实现全局搜索框与结果面板（检索资源与笔记）
  - [ ] 10.2 实现系统设置视图（主题切换）

- [ ] 11. 检查点 - 前端构建与整体验证
  - [ ] 确保 npm run build 与 tauri build 通过,如有疑问请询问用户
