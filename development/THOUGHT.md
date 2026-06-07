# 项目思想与原则

## 核心原则

**所有 API 语义只走一条分发路径。** 无论 transport 是 REST、WebSocket、IPC 还是 in-process 调用，业务逻辑不分散在各入口处。

## 分层规则

1. **Transport adapter 只做格式转换。** 不实现业务规则，不重复分发逻辑。
2. **共享路径处理分发和并发控制。** 包括 experimental gate、序列化 scope、error 转换。
3. **Domain handler 只处理领域逻辑。** 不知道自己被哪个 transport 调用。
4. **职责明确的 crate 边界。** 每层定义自己的抽象，不同层之间通过 trait 对接，不产生循环依赖。

## 演进规则

1. **Legacy 兼容。** 旧接口的 wire format 不因内部重构而改变。新能力走新路径，旧路径保持不变。
2. **可验证的迁移。** 每次替换都要有新旧路径的对比验证，不能只保留一边的测试。
3. **专用的保持专用。** PTY 字节流、MCP 外部协议、daemon 控制语义不纳入 API 统一路径。
4. **协议显式化。** 隐式的 callback side effect（permission、question）改为显式的协议消息，有超时、有降级、有断连清理。

## 当前阶段关注

- 确立共享分发路径作为架构锚点
- transport 盘点、test freeze、legacy 行为锁定
- 确定 crate 间的 trait 边界，而不是 concrete 依赖
- 序列化模型从字符串 key + semaphore 到 typed key + FIFO 的升级路径
