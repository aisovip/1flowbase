---
memory_type: feedback
feedback_category: repository
topic: generic-test-addresses
summary: 回归测试使用示例域名和通用端口，不写用户个人调试地址。
keywords: [tests, fixtures, privacy]
match_when: [把真实部署故障转换为仓库回归测试]
created_at: 2026-10-16 10
updated_at: 2026-10-16 10
last_verified_at: 2026-10-16 10
decision_policy: direct_reference
scope: [tests, fixtures]
---

## 规则
仓库测试使用 .example / .test 保留域名与通用端口。真实用户地址只用于授权的运行态复现及私有证据。

## 原因
用户指出 SSH 代理回归测试写入了其个人公网调试域名；此信息对测试行为无必要。

## 适用场景
从用户环境提炼回归用例时，保留协议、代理层次和状态关系，替换个人环境标识。
