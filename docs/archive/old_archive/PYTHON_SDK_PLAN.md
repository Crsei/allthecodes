# Python SDK 适配计划 — cc-rust

> 基于 `codex/sdk/python/src/codex_app_server` 的架构模式，为 cc-rust 生成 Python SDK。

## 一、架构对比与核心差异

| 维度 | Codex Python SDK | cc-rust Python SDK |
|------|-----------------|-------------------|
| **传输协议** | JSON-RPC v2 over stdio（持久进程） | JSONL over stdio（per-session 或持久进程） |
| **进程模型** | `codex app-server` 常驻子进程 | `claude-code-rs -p --output-format json` |
| **会话抽象** | Thread → Turn → TurnHandle | Session → submit_message → SdkMessage stream |
| **数据模型** | Pydantic（从 JSON Schema 自动生成，6693 行） | 手写 Pydantic（~300 行，对齐 8 种 SdkMessage） |
| **通知系统** | 45+ 双向通知 + request/response | 单向 JSONL 流（8 种 SdkMessage） |
| **并发模型** | 单 turn consumer 互斥锁 | 单 session stdin 写入（每次 run 独立进程） |

## 二、设计原则

1. **遵循 Codex SDK 分层架构**：High-Level API → Client → Transport
2. **与 TypeScript SDK 事件模型对齐**：归一化事件层保持两端一致
3. **协议简单化**：JSONL 单向流，无需 JSON-RPC 握手/request-response 交互
4. **同步优先，异步包装**：sync client 为核心，async 通过 `asyncio.to_thread` 桥接

## 三、总体架构

```
┌──────────────────────────────────────┐
│   High-Level API (api.py)            │
│   ClaudeCode / AsyncClaudeCode       │
│   Session / AsyncSession             │
└────────────────┬─────────────────────┘
                 │
┌────────────────▼─────────────────────┐
│   Client Layer (client.py)           │
│   CcRustClient / AsyncCcRustClient   │
│   JSONL 解析、事件分发、进程管理        │
└────────────────┬─────────────────────┘
                 │
┌────────────────▼─────────────────────┐
│   Transport (subprocess stdio)       │
│   spawn claude-code-rs, read JSONL   │
└──────────────────────────────────────┘
```

## 四、目录结构

```
rust/sdk/python/
├── pyproject.toml                # 包元数据、依赖、构建配置
├── README.md                     # 使用文档
├── src/
│   └── cc_rust_sdk/
│       ├── __init__.py           # 公共 API 导出
│       ├── api.py                # 高层 API: ClaudeCode, Session, RunResult
│       ├── async_api.py          # 异步版本: AsyncClaudeCode, AsyncSession
│       ├── client.py             # 底层客户端: 进程管理、JSONL 读写
│       ├── async_client.py       # 异步客户端 (asyncio.to_thread 包装)
│       ├── models.py             # 数据模型: SdkMessage 各变体的 Pydantic 映射
│       ├── events.py             # 归一化事件: SessionEvent 类型定义
│       ├── transform.py          # 原始 JSONL → SessionEvent 转换逻辑
│       ├── _inputs.py            # 输入类型: TextInput, ImageInput 等
│       ├── _run.py               # RunResult 收集与组装逻辑
│       ├── errors.py             # 异常类层级
│       ├── retry.py              # 重试逻辑 (处理 api_retry 事件)
│       ├── config.py             # CcRustConfig dataclass
│       └── py.typed              # PEP 561 类型标记
└── tests/
    ├── test_client.py            # 客户端单元测试
    ├── test_models.py            # 模型序列化/反序列化测试
    ├── test_transform.py         # 事件转换测试
    └── test_integration.py       # 端到端集成测试
```

## 五、各模块详细设计

### 5.1 `config.py` — 配置

```python
from dataclasses import dataclass, field


@dataclass
class CcRustConfig:
    """cc-rust Python SDK 配置。

    对标 Codex AppServerConfig，但更简单（无需 JSON-RPC 握手参数）。
    """

    # 二进制路径
    bin_path: str | None = None           # claude-code-rs 路径，None 则自动查找
    launch_args_override: tuple[str, ...] | None = None  # 完全覆盖启动参数

    # 运行环境
    cwd: str | None = None                # 工作目录
    env: dict[str, str] | None = None     # 额外环境变量

    # 模型与行为
    model: str | None = None              # 模型覆盖 (e.g. "claude-sonnet-4-20250514")
    permission_mode: str | None = None    # default / auto / bypass / plan
    max_turns: int | None = None          # 最大轮次限制
    max_budget: float | None = None       # 最大预算 (USD)

    # 提示词
    system_prompt: str | None = None      # 自定义系统提示词
    append_system_prompt: str | None = None  # 追加系统提示词

    # 会话
    continue_session: bool = False        # 继续上次会话 (--continue)

    # 认证
    api_key: str | None = None            # ANTHROPIC_API_KEY 注入

    # 调试
    verbose: bool = False
```

**二进制查找优先级**（对齐 TypeScript SDK `exec.ts`）：
1. `bin_path` 显式指定
2. `CC_RUST_PATH` 环境变量
3. `PATH` 中的 `claude-code-rs`
4. 相对路径 `../../target/release/claude-code-rs`
5. 相对路径 `../../target/debug/claude-code-rs`

### 5.2 `models.py` — 数据模型

直接映射 `rust/src/engine/sdk_types.rs` 中的 `SdkMessage` enum。

```python
"""
数据模型 — 对齐 rust/src/engine/sdk_types.rs 的 SdkMessage 各变体。

所有字段使用 snake_case (PEP 8)，与 Rust 侧 serde(rename_all = "snake_case") 一致，
无需 Codex SDK 那样的 camelCase alias 转换。
"""

from __future__ import annotations
from typing import Any, Literal, Union
from pydantic import BaseModel


# ── ContentBlock 变体 ──────────────────────────────────────────────

class TextBlock(BaseModel):
    type: Literal["text"] = "text"
    text: str

class ToolUseBlock(BaseModel):
    type: Literal["tool_use"] = "tool_use"
    id: str
    name: str
    input: Any

class ToolResultBlock(BaseModel):
    type: Literal["tool_result"] = "tool_result"
    tool_use_id: str
    content: Any
    is_error: bool = False

class ThinkingBlock(BaseModel):
    type: Literal["thinking"] = "thinking"
    thinking: str
    signature: str | None = None

class RedactedThinkingBlock(BaseModel):
    type: Literal["redacted_thinking"] = "redacted_thinking"
    data: str

class ImageBlock(BaseModel):
    type: Literal["image"] = "image"
    source: dict[str, Any]

ContentBlock = Union[
    TextBlock, ToolUseBlock, ToolResultBlock,
    ThinkingBlock, RedactedThinkingBlock, ImageBlock,
]


# ── Usage 类型 ─────────────────────────────────────────────────────

class MessageUsage(BaseModel):
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_input_tokens: int = 0
    cache_creation_input_tokens: int = 0

class UsageTracking(BaseModel):
    total_input_tokens: int = 0
    total_output_tokens: int = 0
    total_cache_read_tokens: int = 0
    total_cache_creation_tokens: int = 0
    total_cost_usd: float = 0.0
    api_call_count: int = 0


# ── AssistantMessage ───────────────────────────────────────────────

class AssistantMessageBody(BaseModel):
    uuid: str
    timestamp: int
    role: str = "assistant"
    content: list[ContentBlock]
    usage: MessageUsage | None = None
    stop_reason: str | None = None
    is_api_error_message: bool = False
    api_error: str | None = None
    cost_usd: float = 0.0


# ── CompactMetadata ────────────────────────────────────────────────

class CompactMetadata(BaseModel):
    pre_compact_token_count: int | None = None
    post_compact_token_count: int | None = None


# ── 8 种 SdkMessage 变体 ──────────────────────────────────────────

class SystemInit(BaseModel):
    type: Literal["system_init"] = "system_init"
    tools: list[str]
    model: str
    permission_mode: str
    session_id: str
    uuid: str

class SdkAssistant(BaseModel):
    type: Literal["assistant"] = "assistant"
    message: AssistantMessageBody
    session_id: str
    parent_tool_use_id: str | None = None

class SdkUserReplay(BaseModel):
    type: Literal["user_replay"] = "user_replay"
    content: str
    session_id: str
    uuid: str
    timestamp: int
    is_replay: bool = False
    is_synthetic: bool = False

class SdkStreamEvent(BaseModel):
    type: Literal["stream_event"] = "stream_event"
    event: dict[str, Any]
    session_id: str
    uuid: str

class SdkCompactBoundary(BaseModel):
    type: Literal["compact_boundary"] = "compact_boundary"
    session_id: str
    uuid: str
    compact_metadata: CompactMetadata | None = None

class SdkApiRetry(BaseModel):
    type: Literal["api_retry"] = "api_retry"
    attempt: int
    max_retries: int
    retry_delay_ms: int
    error_status: int | None = None
    error: str
    session_id: str
    uuid: str

class SdkToolUseSummary(BaseModel):
    type: Literal["tool_use_summary"] = "tool_use_summary"
    summary: str
    preceding_tool_use_ids: list[str]
    session_id: str
    uuid: str

class SdkResult(BaseModel):
    type: Literal["result"] = "result"
    subtype: str          # success / error_during_execution / error_max_turns / ...
    is_error: bool
    duration_ms: int
    duration_api_ms: int
    num_turns: int
    result: str
    stop_reason: str | None = None
    session_id: str
    total_cost_usd: float
    usage: UsageTracking
    permission_denials: list[Any] = []
    structured_output: Any | None = None
    uuid: str
    errors: list[str] = []

# 联合类型
SdkMessage = Union[
    SystemInit, SdkAssistant, SdkUserReplay, SdkStreamEvent,
    SdkCompactBoundary, SdkApiRetry, SdkToolUseSummary, SdkResult,
]
```

### 5.3 `events.py` — 归一化事件层

对齐 TypeScript SDK 的 `SessionEvent`，使两个 SDK 对外行为一致。

```python
"""
归一化事件层 — 对齐 rust/sdk/typescript/src/events.ts。

原始 JSONL 的 8 种 SdkMessage 在 transform.py 中被转换为以下 SessionEvent 类型，
SDK 消费者只需关心这一层。
"""

from __future__ import annotations
from dataclasses import dataclass
from typing import Any, Union


# ── Item 类型 ──────────────────────────────────────────────────────

@dataclass
class AgentMessageItem:
    """助手消息（含 content_blocks 和聚合文本）"""
    type: str = "agent_message"
    id: str = ""
    text: str = ""                            # 聚合的文本内容
    content_blocks: list[Any] = None          # 原始 ContentBlock 列表
    usage: dict | None = None
    cost_usd: float = 0.0
    stop_reason: str | None = None
    is_api_error: bool = False

@dataclass
class ToolUseSummaryItem:
    type: str = "tool_use_summary"
    id: str = ""
    summary: str = ""
    preceding_tool_use_ids: list[str] = None

@dataclass
class UserReplayItem:
    type: str = "user_replay"
    id: str = ""
    content: str = ""
    is_replay: bool = False
    is_synthetic: bool = False

@dataclass
class CompactBoundaryItem:
    type: str = "compact_boundary"
    id: str = ""
    pre_compact_tokens: int | None = None
    post_compact_tokens: int | None = None

@dataclass
class ErrorItem:
    type: str = "error"
    id: str = ""
    message: str = ""

SessionItem = Union[
    AgentMessageItem, ToolUseSummaryItem,
    UserReplayItem, CompactBoundaryItem, ErrorItem,
]


# ── Usage 汇总 ─────────────────────────────────────────────────────

@dataclass
class Usage:
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_creation_tokens: int = 0
    total_cost_usd: float = 0.0
    api_call_count: int = 0


# ── TurnError ──────────────────────────────────────────────────────

@dataclass
class TurnError:
    message: str
    subtype: str  # error_during_execution / error_max_turns / error_max_budget_usd


# ── SessionEvent 类型 ──────────────────────────────────────────────

@dataclass
class SessionStarted:
    type: str = "session.started"
    session_id: str = ""
    model: str = ""
    tools: list[str] = None
    permission_mode: str = ""

@dataclass
class TurnStarted:
    type: str = "turn.started"

@dataclass
class ItemCompleted:
    type: str = "item.completed"
    item: SessionItem = None

@dataclass
class StreamDelta:
    type: str = "stream.delta"
    event_type: str = ""
    index: int | None = None
    delta: Any = None
    usage: Any = None
    content_block: Any = None

@dataclass
class TurnCompleted:
    type: str = "turn.completed"
    result: str = ""
    num_turns: int = 0
    duration_ms: int = 0
    total_cost_usd: float = 0.0
    usage: Usage = None

@dataclass
class TurnFailed:
    type: str = "turn.failed"
    error: TurnError = None

@dataclass
class SessionError:
    type: str = "error"
    message: str = ""
    retryable: bool = False
    attempt: int | None = None
    max_retries: int | None = None
    retry_delay_ms: int | None = None

SessionEvent = Union[
    SessionStarted, TurnStarted, ItemCompleted, StreamDelta,
    TurnCompleted, TurnFailed, SessionError,
]
```

### 5.4 `transform.py` — 原始 JSONL → SessionEvent 转换

移植 TypeScript SDK 的 `transform.ts` 逻辑。

```python
"""
转换层 — 将原始 JSONL dict 转为归一化 SessionEvent。

对齐 rust/sdk/typescript/src/transform.ts 的 transformRawEvent()。
"""

from .models import (
    SystemInit, SdkAssistant, SdkUserReplay, SdkStreamEvent,
    SdkCompactBoundary, SdkApiRetry, SdkToolUseSummary, SdkResult,
)
from .events import (
    SessionStarted, ItemCompleted, StreamDelta,
    TurnCompleted, TurnFailed, SessionError, SessionEvent,
    AgentMessageItem, ToolUseSummaryItem, UserReplayItem,
    CompactBoundaryItem, Usage, TurnError,
)


def parse_sdk_message(raw: dict) -> SdkMessage | None:
    """将原始 JSON dict 解析为强类型 SdkMessage。"""
    msg_type = raw.get("type")
    # 按 type 字段分派到对应 Pydantic model
    ...


def transform_raw_event(raw: dict) -> SessionEvent | None:
    """原始 JSONL dict → 归一化 SessionEvent。

    转换规则:
      system_init     → SessionStarted
      assistant       → ItemCompleted(AgentMessageItem)
      user_replay     → ItemCompleted(UserReplayItem)
      stream_event    → StreamDelta
      compact_boundary→ ItemCompleted(CompactBoundaryItem)
      api_retry       → SessionError(retryable=True)
      tool_use_summary→ ItemCompleted(ToolUseSummaryItem)
      result(error)   → TurnFailed
      result(success) → TurnCompleted
    """
    msg_type = raw.get("type")

    match msg_type:
        case "system_init":
            return SessionStarted(
                session_id=raw["session_id"],
                model=raw["model"],
                tools=raw["tools"],
                permission_mode=raw["permission_mode"],
            )

        case "assistant":
            msg = raw["message"]
            # 聚合文本: 拼接所有 text block
            text_parts = [
                b["text"] for b in msg.get("content", [])
                if b.get("type") == "text"
            ]
            return ItemCompleted(item=AgentMessageItem(
                id=msg["uuid"],
                text="".join(text_parts),
                content_blocks=msg.get("content", []),
                usage=msg.get("usage"),
                cost_usd=msg.get("cost_usd", 0.0),
                stop_reason=msg.get("stop_reason"),
                is_api_error=msg.get("is_api_error_message", False),
            ))

        case "user_replay":
            return ItemCompleted(item=UserReplayItem(
                id=raw["uuid"],
                content=raw["content"],
                is_replay=raw.get("is_replay", False),
                is_synthetic=raw.get("is_synthetic", False),
            ))

        case "stream_event":
            evt = raw.get("event", {})
            return StreamDelta(
                event_type=evt.get("type", ""),
                index=evt.get("index"),
                delta=evt.get("delta"),
                usage=evt.get("usage"),
                content_block=evt.get("content_block"),
            )

        case "compact_boundary":
            meta = raw.get("compact_metadata") or {}
            return ItemCompleted(item=CompactBoundaryItem(
                id=raw["uuid"],
                pre_compact_tokens=meta.get("pre_compact_token_count"),
                post_compact_tokens=meta.get("post_compact_token_count"),
            ))

        case "api_retry":
            return SessionError(
                message=raw.get("error", ""),
                retryable=True,
                attempt=raw.get("attempt"),
                max_retries=raw.get("max_retries"),
                retry_delay_ms=raw.get("retry_delay_ms"),
            )

        case "tool_use_summary":
            return ItemCompleted(item=ToolUseSummaryItem(
                id=raw["uuid"],
                summary=raw["summary"],
                preceding_tool_use_ids=raw.get("preceding_tool_use_ids", []),
            ))

        case "result":
            usage_raw = raw.get("usage", {})
            usage = Usage(
                input_tokens=usage_raw.get("total_input_tokens", 0),
                output_tokens=usage_raw.get("total_output_tokens", 0),
                cache_read_tokens=usage_raw.get("total_cache_read_tokens", 0),
                cache_creation_tokens=usage_raw.get("total_cache_creation_tokens", 0),
                total_cost_usd=usage_raw.get("total_cost_usd", 0.0),
                api_call_count=usage_raw.get("api_call_count", 0),
            )
            if raw.get("is_error"):
                return TurnFailed(error=TurnError(
                    message=raw.get("result", ""),
                    subtype=raw.get("subtype", "error_during_execution"),
                ))
            return TurnCompleted(
                result=raw.get("result", ""),
                num_turns=raw.get("num_turns", 0),
                duration_ms=raw.get("duration_ms", 0),
                total_cost_usd=raw.get("total_cost_usd", 0.0),
                usage=usage,
            )

        case _:
            return None  # 未知消息类型，静默忽略
```

### 5.5 `errors.py` — 异常层级

```python
"""
异常层级 — 对标 Codex SDK errors.py 的分层设计。
"""


class CcRustError(Exception):
    """cc-rust SDK 基础异常。"""


class BinaryNotFoundError(CcRustError):
    """找不到 claude-code-rs 二进制文件。"""

    def __init__(self, searched: list[str]):
        self.searched = searched
        super().__init__(
            f"claude-code-rs not found. Searched: {', '.join(searched)}"
        )


class ProcessError(CcRustError):
    """子进程以非零状态码退出。"""

    def __init__(self, exit_code: int, stderr: str):
        self.exit_code = exit_code
        self.stderr = stderr
        super().__init__(f"Process exited with code {exit_code}: {stderr[:500]}")


class TurnExecutionError(CcRustError):
    """SdkResult.is_error=True 时抛出。"""

    def __init__(self, subtype: str, message: str, errors: list[str] | None = None):
        self.subtype = subtype     # error_during_execution / error_max_turns / ...
        self.errors = errors or []
        super().__init__(f"[{subtype}] {message}")


class ParseError(CcRustError):
    """JSONL 行解析失败。"""

    def __init__(self, line: str, cause: Exception):
        self.line = line
        self.cause = cause
        super().__init__(f"Failed to parse JSONL: {cause}")


class TimeoutError(CcRustError):
    """操作超时。"""
```

### 5.6 `_inputs.py` — 输入类型

```python
"""
输入类型 — 对标 Codex SDK _inputs.py。

cc-rust CLI 接受 stdin 纯文本 prompt，因此输入类型主要服务于
高层 API 的类型安全，最终序列化为字符串写入 stdin。
"""

from __future__ import annotations
from dataclasses import dataclass
from typing import Union


@dataclass
class TextInput:
    """纯文本输入。"""
    text: str

@dataclass
class ImageInput:
    """远程图片 URL。"""
    url: str

@dataclass
class LocalImageInput:
    """本地图片文件路径。"""
    path: str


InputItem = Union[TextInput, ImageInput, LocalImageInput]
Input = Union[InputItem, list[InputItem]]
RunInput = Union[Input, str]


def normalize_input(inp: RunInput) -> str:
    """将各种输入类型归一化为 CLI stdin 字符串。

    当前 cc-rust CLI 仅支持纯文本 stdin，
    图片等富类型将在 Rust 侧支持后扩展。
    """
    if isinstance(inp, str):
        return inp
    if isinstance(inp, TextInput):
        return inp.text
    if isinstance(inp, list):
        parts = []
        for item in inp:
            if isinstance(item, TextInput):
                parts.append(item.text)
            elif isinstance(item, (ImageInput, LocalImageInput)):
                parts.append(f"[image: {item.url if hasattr(item, 'url') else item.path}]")
        return "\n".join(parts)
    return str(inp)
```

### 5.7 `client.py` — 底层客户端

```python
"""
底层客户端 — 管理 claude-code-rs 子进程，读取 JSONL 流。

对标 Codex SDK client.py (AppServerClient)，但简化为：
  - 单次 prompt 写入 stdin
  - 逐行读取 stdout JSONL
  - 无 JSON-RPC request/response 交互
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
from collections import deque
from typing import Iterator

from .config import CcRustConfig
from .errors import BinaryNotFoundError, ParseError, ProcessError


class CcRustClient:
    """cc-rust 子进程客户端。"""

    def __init__(self, config: CcRustConfig | None = None):
        self._config = config or CcRustConfig()
        self._process: subprocess.Popen | None = None
        self._stderr_lines: deque[str] = deque(maxlen=400)
        self._stderr_thread: threading.Thread | None = None

    # ── 生命周期 ───────────────────────────────────────────────

    def start(self, prompt: str) -> None:
        """构建 CLI 参数、启动子进程、写入 prompt。"""
        args = self._build_args()
        env = self._build_env()

        self._process = subprocess.Popen(
            args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=self._config.cwd,
            env=env,
        )

        # 后台收集 stderr
        self._stderr_thread = threading.Thread(
            target=self._drain_stderr, daemon=True
        )
        self._stderr_thread.start()

        # 写入 prompt 并关闭 stdin
        self._process.stdin.write(prompt.encode("utf-8"))
        self._process.stdin.close()

    def close(self) -> None:
        """终止子进程: terminate → 2s timeout → kill。"""
        if self._process is None:
            return
        try:
            self._process.terminate()
            self._process.wait(timeout=2.0)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait()
        if self._stderr_thread:
            self._stderr_thread.join(timeout=0.5)
        self._process = None

    # ── JSONL 读取 ─────────────────────────────────────────────

    def read_events(self) -> Iterator[dict]:
        """逐行读取 stdout，yield 解析后的 dict。"""
        assert self._process is not None, "Client not started"
        for raw_line in self._process.stdout:
            line = raw_line.decode("utf-8", errors="replace").strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError as e:
                raise ParseError(line, e)

    def wait(self) -> int:
        """等待子进程结束，返回 exit code。"""
        assert self._process is not None
        return self._process.wait()

    @property
    def stderr_output(self) -> str:
        return "\n".join(self._stderr_lines)

    # ── 内部方法 ───────────────────────────────────────────────

    def _build_args(self) -> list[str]:
        """拼装 CLI 参数列表。"""
        if self._config.launch_args_override:
            return list(self._config.launch_args_override)

        bin_path = self._resolve_binary()
        args = [bin_path, "-p", "--output-format", "json"]

        if self._config.model:
            args += ["--model", self._config.model]
        if self._config.permission_mode:
            args += ["--permission-mode", self._config.permission_mode]
        if self._config.max_turns is not None:
            args += ["--max-turns", str(self._config.max_turns)]
        if self._config.max_budget is not None:
            args += ["--max-budget", str(self._config.max_budget)]
        if self._config.system_prompt:
            args += ["--system-prompt", self._config.system_prompt]
        if self._config.append_system_prompt:
            args += ["--append-system-prompt", self._config.append_system_prompt]
        if self._config.continue_session:
            args.append("--continue")
        if self._config.verbose:
            args.append("--verbose")
        if self._config.cwd:
            args += ["--cwd", self._config.cwd]

        return args

    def _build_env(self) -> dict[str, str]:
        """构建子进程环境变量。"""
        env = dict(os.environ)
        env["CC_RUST_INTERNAL_ORIGINATOR"] = "cc_rust_sdk_python"
        if self._config.api_key:
            env["ANTHROPIC_API_KEY"] = self._config.api_key
        if self._config.env:
            env.update(self._config.env)
        return env

    def _resolve_binary(self) -> str:
        """查找 claude-code-rs 二进制文件。"""
        searched = []

        # 1. 显式指定
        if self._config.bin_path:
            return self._config.bin_path

        # 2. 环境变量
        env_path = os.environ.get("CC_RUST_PATH")
        if env_path:
            return env_path

        # 3. PATH 查找
        which = shutil.which("claude-code-rs")
        if which:
            return which
        searched.append("PATH")

        # 4. 相对路径 (开发模式)
        for rel in [
            "../../target/release/claude-code-rs",
            "../../target/debug/claude-code-rs",
        ]:
            abs_path = os.path.normpath(
                os.path.join(os.path.dirname(__file__), rel)
            )
            if os.path.isfile(abs_path):
                return abs_path
            searched.append(abs_path)

        raise BinaryNotFoundError(searched)

    def _drain_stderr(self) -> None:
        """后台线程: 持续读取 stderr 到缓冲区。"""
        assert self._process is not None
        for line in self._process.stderr:
            self._stderr_lines.append(
                line.decode("utf-8", errors="replace").rstrip()
            )
```

### 5.8 `async_client.py` — 异步客户端

```python
"""
异步客户端 — 对标 Codex SDK async_client.py。

通过 asyncio.to_thread 包装同步 CcRustClient，
使用 asyncio.Lock 保证并发安全。
"""

from __future__ import annotations

import asyncio
from typing import AsyncIterator

from .client import CcRustClient
from .config import CcRustConfig


class AsyncCcRustClient:
    """异步 cc-rust 客户端。"""

    def __init__(self, config: CcRustConfig | None = None):
        self._sync = CcRustClient(config)
        self._lock = asyncio.Lock()

    async def start(self, prompt: str) -> None:
        async with self._lock:
            await asyncio.to_thread(self._sync.start, prompt)

    async def read_events(self) -> AsyncIterator[dict]:
        """异步逐行读取 JSONL。"""
        loop = asyncio.get_event_loop()
        queue: asyncio.Queue[dict | None] = asyncio.Queue()

        def _reader():
            try:
                for event in self._sync.read_events():
                    loop.call_soon_threadsafe(queue.put_nowait, event)
            finally:
                loop.call_soon_threadsafe(queue.put_nowait, None)

        reader_task = loop.run_in_executor(None, _reader)

        while True:
            item = await queue.get()
            if item is None:
                break
            yield item

        await reader_task

    async def close(self) -> None:
        async with self._lock:
            await asyncio.to_thread(self._sync.close)

    async def wait(self) -> int:
        return await asyncio.to_thread(self._sync.wait)
```

### 5.9 `_run.py` — RunResult 收集

```python
"""
RunResult 收集逻辑 — 对标 Codex SDK _run.py。

从 SessionEvent 流中提取最终结果。
"""

from __future__ import annotations
from dataclasses import dataclass, field
from typing import Iterator, AsyncIterator

from .events import (
    SessionEvent, SessionStarted, ItemCompleted, TurnCompleted,
    TurnFailed, AgentMessageItem, Usage,
)
from .errors import TurnExecutionError


@dataclass
class RunResult:
    """单次 run 的最终结果。"""

    final_response: str | None = None       # 最后一条助手文本
    items: list[ItemCompleted] = field(default_factory=list)
    usage: Usage | None = None
    session_id: str | None = None
    num_turns: int = 0
    duration_ms: int = 0
    total_cost_usd: float = 0.0


def collect_run_result(
    events: Iterator[SessionEvent],
    *,
    raise_on_error: bool = True,
) -> RunResult:
    """从同步事件流收集 RunResult。"""
    result = RunResult()

    for event in events:
        if isinstance(event, SessionStarted):
            result.session_id = event.session_id

        elif isinstance(event, ItemCompleted):
            result.items.append(event)
            # 记录最后一条助手消息文本
            if isinstance(event.item, AgentMessageItem) and event.item.text:
                result.final_response = event.item.text

        elif isinstance(event, TurnCompleted):
            result.usage = event.usage
            result.num_turns = event.num_turns
            result.duration_ms = event.duration_ms
            result.total_cost_usd = event.total_cost_usd
            if event.result:
                result.final_response = event.result

        elif isinstance(event, TurnFailed):
            if raise_on_error:
                raise TurnExecutionError(
                    subtype=event.error.subtype,
                    message=event.error.message,
                )

    return result


async def collect_async_run_result(
    events: AsyncIterator[SessionEvent],
    *,
    raise_on_error: bool = True,
) -> RunResult:
    """从异步事件流收集 RunResult。"""
    result = RunResult()

    async for event in events:
        if isinstance(event, SessionStarted):
            result.session_id = event.session_id
        elif isinstance(event, ItemCompleted):
            result.items.append(event)
            if isinstance(event.item, AgentMessageItem) and event.item.text:
                result.final_response = event.item.text
        elif isinstance(event, TurnCompleted):
            result.usage = event.usage
            result.num_turns = event.num_turns
            result.duration_ms = event.duration_ms
            result.total_cost_usd = event.total_cost_usd
            if event.result:
                result.final_response = event.result
        elif isinstance(event, TurnFailed):
            if raise_on_error:
                raise TurnExecutionError(
                    subtype=event.error.subtype,
                    message=event.error.message,
                )

    return result
```

### 5.10 `api.py` — 高层同步 API

```python
"""
高层同步 API — 对标 Codex SDK api.py (Codex, Thread, TurnHandle)。

简化为 ClaudeCode + Session 两级，因为 cc-rust 无 thread 概念。
"""

from __future__ import annotations
from typing import Iterator

from .config import CcRustConfig
from .client import CcRustClient
from .transform import transform_raw_event
from .events import SessionEvent
from ._inputs import RunInput, normalize_input
from ._run import RunResult, collect_run_result


class ClaudeCode:
    """cc-rust Python SDK 主入口 (同步)。

    用法:
        claude = ClaudeCode(CcRustConfig(model="claude-sonnet-4-20250514"))
        result = claude.run("Explain this codebase")
        print(result.final_response)
    """

    def __init__(self, config: CcRustConfig | None = None):
        self._config = config or CcRustConfig()

    def run(self, prompt: RunInput, *, raise_on_error: bool = True) -> RunResult:
        """发送 prompt，阻塞直到完成，返回 RunResult。"""
        events = self.run_streamed(prompt)
        return collect_run_result(events, raise_on_error=raise_on_error)

    def run_streamed(self, prompt: RunInput) -> Iterator[SessionEvent]:
        """发送 prompt，yield 归一化事件流。"""
        text = normalize_input(prompt)
        client = CcRustClient(self._config)
        client.start(text)
        try:
            for raw in client.read_events():
                event = transform_raw_event(raw)
                if event is not None:
                    yield event
        finally:
            client.close()

    def session(self) -> "Session":
        """创建有状态会话 (支持多轮对话)。"""
        return Session(self)


class Session:
    """有状态会话 — 支持 --continue 多轮对话。

    用法:
        claude = ClaudeCode()
        session = claude.session()

        r1 = session.submit("Read main.rs")
        r2 = session.submit("Now explain the query loop")  # 自动继续
    """

    def __init__(self, claude: ClaudeCode):
        self._claude = claude
        self._session_id: str | None = None
        self._turn_count: int = 0

    @property
    def session_id(self) -> str | None:
        return self._session_id

    def submit(self, prompt: RunInput, *, raise_on_error: bool = True) -> RunResult:
        """提交消息到当前会话。"""
        # 首次之后启用 --continue
        if self._turn_count > 0:
            self._claude._config.continue_session = True

        result = self._claude.run(prompt, raise_on_error=raise_on_error)
        self._turn_count += 1

        if result.session_id:
            self._session_id = result.session_id

        return result

    def submit_streamed(self, prompt: RunInput) -> Iterator[SessionEvent]:
        """提交消息并返回事件流。"""
        if self._turn_count > 0:
            self._claude._config.continue_session = True
        self._turn_count += 1
        return self._claude.run_streamed(prompt)
```

### 5.11 `async_api.py` — 异步高层 API

```python
"""
异步高层 API — 对标 Codex SDK api.py 的 AsyncCodex / AsyncThread。
"""

from __future__ import annotations

import asyncio
from typing import AsyncIterator

from .config import CcRustConfig
from .async_client import AsyncCcRustClient
from .transform import transform_raw_event
from .events import SessionEvent
from ._inputs import RunInput, normalize_input
from ._run import RunResult, collect_async_run_result


class AsyncClaudeCode:
    """cc-rust Python SDK 主入口 (异步)。

    用法:
        async with AsyncClaudeCode() as claude:
            result = await claude.run("Explain this codebase")
            print(result.final_response)
    """

    def __init__(self, config: CcRustConfig | None = None):
        self._config = config or CcRustConfig()

    async def __aenter__(self) -> "AsyncClaudeCode":
        return self

    async def __aexit__(self, *exc) -> None:
        pass  # 无持久进程需要清理

    async def run(
        self, prompt: RunInput, *, raise_on_error: bool = True
    ) -> RunResult:
        """发送 prompt，await 直到完成。"""
        events = self.run_streamed(prompt)
        return await collect_async_run_result(
            events, raise_on_error=raise_on_error
        )

    async def run_streamed(self, prompt: RunInput) -> AsyncIterator[SessionEvent]:
        """发送 prompt，yield 归一化事件流。"""
        text = normalize_input(prompt)
        client = AsyncCcRustClient(self._config)
        await client.start(text)
        try:
            async for raw in client.read_events():
                event = transform_raw_event(raw)
                if event is not None:
                    yield event
        finally:
            await client.close()

    def session(self) -> "AsyncSession":
        return AsyncSession(self)


class AsyncSession:
    """异步有状态会话。"""

    def __init__(self, claude: AsyncClaudeCode):
        self._claude = claude
        self._session_id: str | None = None
        self._turn_count: int = 0

    @property
    def session_id(self) -> str | None:
        return self._session_id

    async def submit(
        self, prompt: RunInput, *, raise_on_error: bool = True
    ) -> RunResult:
        if self._turn_count > 0:
            self._claude._config.continue_session = True
        result = await self._claude.run(prompt, raise_on_error=raise_on_error)
        self._turn_count += 1
        if result.session_id:
            self._session_id = result.session_id
        return result

    async def submit_streamed(self, prompt: RunInput) -> AsyncIterator[SessionEvent]:
        if self._turn_count > 0:
            self._claude._config.continue_session = True
        self._turn_count += 1
        async for event in self._claude.run_streamed(prompt):
            yield event
```

### 5.12 `__init__.py` — 公共导出

```python
"""cc-rust Python SDK"""

__version__ = "0.1.0"

from .api import ClaudeCode, Session
from .async_api import AsyncClaudeCode, AsyncSession
from .config import CcRustConfig
from ._run import RunResult
from .events import (
    SessionEvent, SessionStarted, ItemCompleted, StreamDelta,
    TurnCompleted, TurnFailed, SessionError,
    AgentMessageItem, ToolUseSummaryItem, UserReplayItem,
    CompactBoundaryItem, Usage,
)
from .errors import (
    CcRustError, BinaryNotFoundError, ProcessError,
    TurnExecutionError, ParseError,
)
from ._inputs import TextInput, ImageInput, LocalImageInput

__all__ = [
    # 高层 API
    "ClaudeCode", "AsyncClaudeCode",
    "Session", "AsyncSession",
    "CcRustConfig", "RunResult",
    # 事件
    "SessionEvent", "SessionStarted", "ItemCompleted", "StreamDelta",
    "TurnCompleted", "TurnFailed", "SessionError",
    # Item 类型
    "AgentMessageItem", "ToolUseSummaryItem", "UserReplayItem",
    "CompactBoundaryItem", "Usage",
    # 输入类型
    "TextInput", "ImageInput", "LocalImageInput",
    # 异常
    "CcRustError", "BinaryNotFoundError", "ProcessError",
    "TurnExecutionError", "ParseError",
]
```

### 5.13 `retry.py` — 重试逻辑

```python
"""
重试逻辑 — 对标 Codex SDK retry.py。

处理 api_retry 事件时的客户端侧等待策略。
注意: cc-rust 的 api_retry 是通知性的（Rust 侧已自行重试），
此模块仅用于 SDK 层面可能需要的外部重试包装。
"""

from __future__ import annotations

import random
import time
from typing import Callable, TypeVar

from .errors import CcRustError

T = TypeVar("T")


class RetryLimitExceeded(CcRustError):
    """重试次数耗尽。"""

    def __init__(self, attempts: int, last_error: Exception):
        self.attempts = attempts
        self.last_error = last_error
        super().__init__(f"Retry limit exceeded after {attempts} attempts: {last_error}")


def retry_on_failure(
    op: Callable[[], T],
    *,
    max_attempts: int = 3,
    initial_delay_s: float = 0.5,
    max_delay_s: float = 5.0,
    jitter_ratio: float = 0.2,
    retryable: Callable[[Exception], bool] = lambda e: True,
) -> T:
    """带指数退避 + 抖动的重试包装器。"""
    delay = initial_delay_s

    for attempt in range(1, max_attempts + 1):
        try:
            return op()
        except Exception as e:
            if attempt == max_attempts or not retryable(e):
                raise RetryLimitExceeded(attempt, e) from e
            jitter = delay * jitter_ratio * (2 * random.random() - 1)
            time.sleep(min(delay + jitter, max_delay_s))
            delay = min(delay * 2, max_delay_s)

    raise RuntimeError("unreachable")  # type: ignore
```

## 六、与 Codex SDK 的关键适配差异总结

| Codex SDK 特性 | cc-rust 适配策略 | 原因 |
|---------------|-----------------|------|
| JSON-RPC v2 请求/响应 | 不需要 | JSONL 是单向流 |
| `initialize` 握手 | `system_init` JSONL 消息替代 | 无需双向握手 |
| Thread / Turn 两级抽象 | Session 单级 | Rust 无持久 thread 概念 |
| 45+ 通知类型 | 8 种 SdkMessage → 7 种 SessionEvent | 协议更简单 |
| `generated/v2_all.py` (6693 行) | 手写 ~300 行 models.py | 类型少得多 |
| Approval Handler 回调 | `permission_mode` CLI 参数 | Rust 侧处理审批 |
| Turn consumer 互斥锁 | 不需要 | 每次 run 独立进程 |
| `thread/list`, `thread/fork` | `--continue` + 文件系统 session | 无服务端状态 |
| camelCase ↔ snake_case 转换 | 不需要 | Rust serde 输出即 snake_case |

## 七、实施阶段

| 阶段 | 内容 | 产出文件 | 依赖 |
|------|------|---------|------|
| **Phase 1: 基础层** | 配置、数据模型、异常 | `config.py`, `models.py`, `errors.py` | 无 |
| **Phase 2: 传输层** | 进程管理、JSONL 读写 | `client.py` | Phase 1 |
| **Phase 3: 转换层** | 事件归一化 | `events.py`, `transform.py` | Phase 1 |
| **Phase 4: 同步 API** | 高层同步接口 | `api.py`, `_inputs.py`, `_run.py` | Phase 2 + 3 |
| **Phase 5: 异步 API** | 高层异步接口 | `async_client.py`, `async_api.py` | Phase 4 |
| **Phase 6: 打包发布** | 导出、类型标记、包配置 | `__init__.py`, `py.typed`, `pyproject.toml` | Phase 5 |
| **Phase 7: 测试** | 单元测试 + 集成测试 | `tests/` | Phase 6 |

## 八、Rust 侧可能需要的改动

### 8.1 持久进程模式（优先级: 中）

当前 Rust CLI 是单次 prompt 执行。若需支持 Codex 风格的多轮对话 SDK，需在 Rust 侧添加 stdin 循环模式：

```
claude-code-rs --interactive --output-format json
```

行为: 读一行 prompt → 输出 JSONL 事件流 → 输出 result → 等待下一行。

### 8.2 JSONL 输出稳定性（优先级: 高）

确保所有 8 种 SdkMessage 的 JSON 序列化格式稳定，特别关注:
- `ContentBlock` 联合类型的 tag 字段
- `Usage` / `UsageTracking` 的字段命名一致性
- `None` / `null` 值的序列化行为

### 8.3 进程优雅终止（优先级: 高）

确保 SIGTERM / stdin EOF 时:
- 正确 flush 最后的 `result` 消息
- stdout 缓冲区完整写出
- 非零退出码仅用于真正的错误

### 8.4 错误输出规范化（优先级: 低）

stderr 输出格式标准化，便于 SDK 解析错误信息。

## 九、使用示例

### 基础用法

```python
from cc_rust_sdk import ClaudeCode, CcRustConfig

# 简单调用
claude = ClaudeCode()
result = claude.run("Explain what main.rs does")
print(result.final_response)
print(f"Cost: ${result.total_cost_usd:.4f}")
```

### 流式输出

```python
from cc_rust_sdk import ClaudeCode, ItemCompleted, AgentMessageItem

claude = ClaudeCode()
for event in claude.run_streamed("Write a hello world in Rust"):
    if isinstance(event, ItemCompleted):
        if isinstance(event.item, AgentMessageItem):
            print(event.item.text, end="", flush=True)
```

### 多轮会话

```python
from cc_rust_sdk import ClaudeCode

claude = ClaudeCode()
session = claude.session()

r1 = session.submit("Read src/main.rs")
print(r1.final_response)

r2 = session.submit("Now explain the bootstrap phase")
print(r2.final_response)
```

### 异步用法

```python
import asyncio
from cc_rust_sdk import AsyncClaudeCode, CcRustConfig

async def main():
    config = CcRustConfig(
        model="claude-sonnet-4-20250514",
        permission_mode="bypass",
        max_turns=5,
    )
    async with AsyncClaudeCode(config) as claude:
        result = await claude.run("Summarize this project")
        print(result.final_response)

asyncio.run(main())
```

### 自定义配置

```python
from cc_rust_sdk import ClaudeCode, CcRustConfig

config = CcRustConfig(
    bin_path="/usr/local/bin/claude-code-rs",
    cwd="/path/to/project",
    model="claude-sonnet-4-20250514",
    permission_mode="auto",
    max_turns=10,
    max_budget=0.50,
    api_key="sk-ant-...",
    system_prompt="You are a code reviewer. Be concise.",
    verbose=True,
)

claude = ClaudeCode(config)
result = claude.run("Review the last commit")
```

### 错误处理

```python
from cc_rust_sdk import (
    ClaudeCode, CcRustError, BinaryNotFoundError,
    TurnExecutionError, ProcessError,
)

try:
    claude = ClaudeCode()
    result = claude.run("Do something")
except BinaryNotFoundError as e:
    print(f"Install claude-code-rs first. Searched: {e.searched}")
except TurnExecutionError as e:
    print(f"Turn failed [{e.subtype}]: {e}")
except ProcessError as e:
    print(f"Process crashed (exit {e.exit_code}): {e.stderr}")
except CcRustError as e:
    print(f"SDK error: {e}")
```
