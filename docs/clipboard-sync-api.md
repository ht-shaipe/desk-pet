# 剪切板云同步接口文档

> 同账号多设备剪切板同步功能，复用现有 `hub.htui.cc` 鉴权体系。

## 1. 概述

| 项目 | 说明 |
|------|------|
| 基地址 | `https://hub.htui.cc` |
| API 前缀 | `/api/cms/clip` |
| 鉴权方式 | `Authorization: <token>`（裸 token，不带 Bearer，与收藏同步一致） |
| 内容类型 | `application/json` |
| 时间格式 | BIGINT 毫秒 Unix 时间戳（所有 `_time` 字段统一） |
| 同步模式 | 增量同步：push 推送变更 + pull 游标拉取 |

### 客户端约束

| 约束 | 值 |
|------|-----|
| 单条内容上限 | 4000 字符 |
| 每用户条目上限 | 500 条（服务端同样维护） |
| 条目 ID 格式 | `c-{毫秒时间戳}`，如 `c-1730000000000` |
| 去重策略 | 同用户全局去重，相同内容只保留最新一条 |

---

## 2. 表结构

```sql
CREATE TABLE clip_item (
    id            BIGINT       AUTO_INCREMENT PRIMARY KEY COMMENT '服务端自增ID',
    user_id       BIGINT       NOT NULL                  COMMENT '用户ID（关联现有user表）',
    client_id     VARCHAR(64)  NOT NULL                  COMMENT '客户端条目ID，格式 c-{毫秒时间戳}',
    content       TEXT         NOT NULL                  COMMENT '文本内容（≤4000字符）',
    content_hash  CHAR(64)     NOT NULL                  COMMENT 'SHA-256(content)，用于去重',
    create_time   BIGINT       NOT NULL                  COMMENT '客户端创建时间（毫秒时间戳）',
    update_time   BIGINT       NOT NULL                  COMMENT '客户端更新时间（毫秒时间戳）',
    delete_time   BIGINT       NULL DEFAULT NULL         COMMENT '软删除时间，NULL=未删除',
    server_time   BIGINT       NOT NULL                  COMMENT '服务端记录时间（增量游标）',

    UNIQUE KEY uk_user_client        (user_id, client_id),
    KEY        idx_user_server_time  (user_id, server_time),
    KEY        idx_user_hash_alive   (user_id, content_hash, delete_time)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='剪切板同步条目';
```

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|:----:|------|
| `id` | BIGINT | 是 | 服务端主键，自增 |
| `user_id` | BIGINT | 是 | 关联现有用户表，通过 token 解析获取 |
| `client_id` | VARCHAR(64) | 是 | 客户端生成的条目 ID（`c-{毫秒}`），`(user_id, client_id)` 联合唯一 |
| `content` | TEXT | 是 | 文本内容，服务端校验 ≤ 4000 字符 |
| `content_hash` | CHAR(64) | 是 | `SHA-256(content)`，服务端计算，用于快速去重 |
| `create_time` | BIGINT | 是 | 客户端创建时间（毫秒时间戳） |
| `update_time` | BIGINT | 是 | 客户端更新时间（毫秒时间戳） |
| `delete_time` | BIGINT | 否 | 软删除时间，NULL 表示未删除。多设备同步必须 |
| `server_time` | BIGINT | 是 | 服务端写入时间，客户端用它做下次增量拉取游标 |

### 索引说明

| 索引名 | 字段 | 用途 |
|--------|------|------|
| `uk_user_client` | `(user_id, client_id)` | 联合唯一约束，保证同用户下条目 ID 不重复 |
| `idx_user_server_time` | `(user_id, server_time)` | 增量拉取查询 |
| `idx_user_hash_alive` | `(user_id, content_hash, delete_time)` | 去重查询 |

---

## 3. 接口定义

### 3.1 推送 — `POST /api/cms/clip/push`

客户端将本地有变更的条目批量上传（新增、修改、删除统一走此接口）。

#### 请求

```jsonc
POST /api/cms/clip/push
Authorization: <token>
Content-Type: application/json

{
  "items": [
    {
      "client_id": "c-1730000000000",
      "content": "hello world",
      "create_time": 1730000000000,
      "update_time": 1730000000000,
      "deleted": false
    },
    {
      "client_id": "c-1730000000001",
      "content": "",
      "create_time": 1730000000001,
      "update_time": 1730000000001,
      "deleted": true
    }
  ]
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|:----:|------|
| `items` | Array | 是 | 变更条目列表 |
| `items[].client_id` | String | 是 | 客户端条目 ID |
| `items[].content` | String | 是 | 文本内容（`deleted=true` 时可传空串） |
| `items[].create_time` | Long | 是 | 创建时间（毫秒时间戳） |
| `items[].update_time` | Long | 是 | 更新时间（毫秒时间戳） |
| `items[].deleted` | Boolean | 是 | 是否删除 |

#### 响应

```jsonc
{
  "code": 200,
  "result": {
    "server_time": 1730000000500,
    "results": [
      { "client_id": "c-1730000000000", "status": "upserted" },
      { "client_id": "c-1730000000001", "status": "deleted"  }
    ]
  }
}
```

| 字段 | 说明 |
|------|------|
| `server_time` | 服务端处理完成时间，客户端存为同步游标 |
| `results[].status` | `upserted`（新增/更新）/ `deleted`（已软删除）/ `deduped`（因去重被合并） |

#### 错误码

| code | 说明 |
|------|------|
| 200 | 成功 |
| 401 | 未授权 / token 无效 |
| 400 | 参数错误（如 content 超 4000 字符） |

---

### 3.2 拉取 — `POST /api/cms/clip/pull`

基于游标增量拉取，游标为上次拉取返回的 `server_time`。首次传 `0` 拉全量。

#### 请求

```jsonc
POST /api/cms/clip/pull
Authorization: <token>
Content-Type: application/json

{
  "since": 1729999000000,
  "limit": 100
}
```

| 参数 | 类型 | 必填 | 默认 | 说明 |
|------|------|:----:|------|------|
| `since` | Long | 是 | — | 游标，拉取 `server_time > since` 的变更 |
| `limit` | Integer | 否 | 100 | 单次最大条数，建议 ≤ 200 |

#### 响应

```jsonc
{
  "code": 200,
  "result": {
    "server_time": 1730000000500,
    "items": [
      {
        "client_id": "c-1730000000000",
        "content": "hello world",
        "create_time": 1730000000000,
        "update_time": 1730000000000,
        "delete_time": null
      },
      {
        "client_id": "c-1730000000001",
        "content": "",
        "create_time": 1730000000001,
        "update_time": 1730000000001,
        "delete_time": 1730000000001
      }
    ],
    "has_more": false
  }
}
```

| 字段 | 说明 |
|------|------|
| `server_time` | 本次拉取的最大 `server_time`，客户端存为下次游标 |
| `items[].delete_time` | 非 null 表示该条目已被删除，客户端应同步删除本地对应条目 |
| `has_more` | 是否还有更多数据，客户端应循环拉取直到 `false` |

---

## 4. 服务端处理逻辑

### 4.1 推送处理（逐条 upsert）

```
对请求中每个 item：

1. 解析 token → user_id（鉴权失败返回 401）

2. 校验 content 长度 ≤ 4000，超长返回 400

3. 若 deleted = true：
     UPDATE clip_item
       SET delete_time = <now>, server_time = <now>
     WHERE user_id = ? AND client_id = ? AND delete_time IS NULL
     → status = "deleted"

4. 若 deleted = false：
     a. 计算 content_hash = SHA-256(content)
     b. 查询同 user_id 下是否有 content_hash 相同且未删除的条目：
          SELECT id, client_id FROM clip_item
          WHERE user_id = ? AND content_hash = ? AND delete_time IS NULL
     c. 若存在相同 hash 的旧条目（且 client_id 不同）：
          - 将旧条目软删除：UPDATE SET delete_time = <now>, server_time = <now>
          - 插入新条目
          → status = "deduped"
     d. 若不存在：
          INSERT INTO clip_item (...) VALUES (...)
          ON DUPLICATE KEY UPDATE
            content = VALUES(content),
            content_hash = VALUES(content_hash),
            update_time = VALUES(update_time),
            delete_time = NULL,
            server_time = <now>
          → status = "upserted"

5. 维护容量上限：
     查询该 user_id 未删除条目数
     若 > 500，按 update_time ASC 软删除最旧的（逐条 SET delete_time = <now>, server_time = <now>）
```

### 4.2 拉取处理

```sql
SELECT client_id, content, create_time, update_time, delete_time, server_time
FROM clip_item
WHERE user_id = ? AND server_time > ?
ORDER BY server_time ASC
LIMIT ?;
```

- `has_more` = 查询结果数 == limit 时为 true
- 返回的 `server_time` = 本次结果中最大的 `server_time`（无结果时返回当前服务器时间）

### 4.3 去重策略

- **同用户全局去重**：相同 `content_hash` 只保留一条（最新的），旧的软删除
- 与客户端本地去重逻辑一致
- 去重查询利用索引 `idx_user_hash_alive`

---

## 5. 客户端同步流程（供参考）

```
触发时机：启动 / 打开剪切板面板 / 定时（如 5 分钟）/ 手动触发

1. Push 阶段：
   - 收集本地所有条目（首次全量，后续可只推变更）
   - POST /api/cms/clip/push
   - 记入返回的 server_time

2. Pull 阶段：
   - 读取本地存储的 last_sync_server_time（默认 0）
   - 循环 POST /api/cms/clip/pull，直到 has_more = false
   - 合并逻辑：
     - delete_time 非 null → 删除本地对应 client_id 的条目
     - delete_time 为 null → upsert 到本地（按 client_id 匹配）
   - 更新本地 last_sync_server_time = 最终响应的 server_time

3. 去重合并：
   - 拉取后本地执行全局去重（相同内容只保留最新）
   - 与服务端去重逻辑保持一致
```

---

## 6. 注意事项

| 事项 | 说明 |
|------|------|
| 鉴权 | 复用现有 Authorization 裸 token，与收藏同步 `/api/cms/favorite/*` 一致 |
| 时间格式 | 所有 `_time` 字段统一 BIGINT 毫秒时间戳，避免时区问题 |
| 容量限制 | 服务端维护 500 条上限，按 `update_time` 升序软删除最旧条目 |
| 内容校验 | 服务端校验 `content` 长度 ≤ 4000 字符 |
| 幂等性 | 多设备同时推送相同 `client_id`，`ON DUPLICATE KEY UPDATE` 保证幂等 |
| 软删除 | 多设备同步必须用软删除，否则设备 A 删除的条目会被设备 B 推送时恢复 |
| 字符集 | `utf8mb4`，支持 emoji 等四字节字符 |
