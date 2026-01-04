# report（Markdown）

`macdiet report --markdown` は、Issue/PR 等に貼り付けやすい要約を出力します。

```sh
macdiet report --markdown
macdiet report --markdown --include-evidence
```

## 出力例（抜粋）

※ `generated_at` / サイズ見積もりは環境・実行タイミングで変動します。

```md
# macdiet report

- tool_version: 0.1.0
- generated_at: 2026-01-02T00:00:00Z
- os: macOS 26.0
- estimated_total: 12.3 GB
- unobserved: 512 MB

## Findings (1)

### npm cache: ~/.npm (est: 1.2 GB)
- id: `npm-cache`
- risk: R1
- confidence: 0.90
- recommended_actions:
  - `npm-cache-trash`

## Actions (1)

### Move npm cache to Trash (R1) (est: 1.2 GB)
- id: `npm-cache-trash`
- risk: R1
- targets:
  - `npm-cache`
- kind: TRASH_MOVE
- paths:
  - `~/.npm`
- impact:
  - Impact: `npm install` may take longer next time due to re-downloading packages.
```

