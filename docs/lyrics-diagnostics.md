# QQ 音乐与网易云音乐歌词排查日志

Mac 日志位置：`~/Library/Logs/Poni/poni.log`。日志中搜索 `lyrics diag`，再用相同的 `player`、`request` 和 `attempt` 串联一次查询。`attempt=0` 为首次查询，后续为原有的自动重试，不新增请求或重试次数。

记录内容包括当前播放器提供的歌名、歌手、专辑、时长；网易云仅记录解析成功的数字歌曲 ID，不记录原始 content_id。每个元数据字段最多 120 字符，并转义换行和控制字符。候选详情每轮最多 3 条；每种排除原因的计数完整记录。只在换曲、查询或原有重试时写入，不随歌词逐句更新或播放进度刷日志。

| 阶段/原因 | 含义 |
| --- | --- |
| `enqueue / lyrics_disabled` | 歌词设置关闭，当前换曲没有发起请求 |
| `enqueue / missing_title` | 播放器未提供歌名，当前换曲没有发起请求 |
| `enqueue / worker_unavailable` | 歌词工作线程未启动 |
| `metadata / insufficient_metadata` | 歌手缺失，或专辑和时长同时缺失，匹配提前停止 |
| `cache / hit` | 使用已校验的缓存歌词 |
| `cache / miss_or_rejected` | 无可用缓存，继续在线查询 |
| `lookup / player_song_id` | 网易云使用播放器给出的可信歌曲 ID |
| `lookup / metadata_search` | 网易云没有可信歌曲 ID，改用原有元数据搜索 |
| `search` | 记录检索轮次、候选总数、通过校验且去重后的数量及各类排除计数 |
| `title_or_version_mismatch` | 歌名或 Live 等版本信息不一致 |
| `artist_mismatch` | 歌手不一致 |
| `candidate_duration_missing` | 当前歌有时长，但候选没有有效时长 |
| `duration_out_of_range` | 时长差超过原有阈值：同专辑使用既有同步阈值，跨专辑最多 2 秒 |
| `album_mismatch_without_duration` | 当前歌无时长，无法用相同专辑证明录音一致 |
| `selection` | 候选选定、同专辑歧义、跨专辑不唯一或无合格候选 |
| `song_detail / requested_id_missing` | 网易云歌曲详情未返回指定 ID |
| `album_search / no_exact_album_and_artist` | QQ 专辑补查没有同名、同歌手的专辑 |
| `album_search / ambiguous_album` | QQ 专辑补查结果不唯一 |
| `album_tracks / incomplete_track_list` | QQ 专辑曲目表不完整，不能据此确定录音 |
| `lyric / empty_lyric` | QQ 返回空歌词字段 |
| `lyric / provider_no_lyric` | 网易云声明无歌词或尚未收录 |
| `lyric / missing_timestamps_or_too_few_lines` | 主歌词缺少有效同步时间戳，或有效文字行数未达既有要求 |
| `http_status` / `api_code` / `service_code` | HTTP 状态和平台业务码，不记录响应正文 |
| `timeout` / `network_error` / `client_error` | 请求超时、连接失败或客户端错误，不记录可能包含 URL 的底层错误文本 |
| `rate_limit_backoff` / `retry` | 平台限流等待、是否安排重试及等待时间 |
| `finish` | 本次结果、同步标记/文字行数或失败原因、耗时；取消与未匹配区分 |

日志只保存在本机原有日志文件，不上传。不记录歌词正文、完整请求 URL、HTTP 请求或响应正文、Cookie、令牌或邮箱凭据。歌名等听歌信息会进入本机日志，向他人分享前可按需删去。

匹配规则、缓存、来源对应、重试、界面提示均保持原有行为；此次只增加诊断。图标另外修正了已选中标签在鼠标悬停时变白的问题，选中图标与文字保持相同强调色。
