# E118c unexecuted Parity draft note

Status: **UNEXECUTED DRAFT; NOT A PREREGISTRATION OR SCIENTIFIC RESULT**

This note preserves the only useful repository-pool detail from three
uncommitted E118c drafts that were mistakenly started in the unrelated
`advatar/parity` repository. Those drafts were never committed or pushed, no
E118c candidate outcome was generated or inspected, and their implementation
code was untested. Preserving this note does not authorize or begin E118c.

The draft's preliminary, outcome-blind scan of the E118-pinned
`SWE-bench/SWE-bench_Multilingual` dataset identified 41 repositories. It
quarantined all 13 repositories touched by E118 or E118a:

- E118 train / E118a target: `babel/babel`, `prometheus/prometheus`,
  `sharkdp/bat`, `tokio-rs/axum`, `tokio-rs/tokio`, `vuejs/core`;
- E118 probe / E118a sensor source: `axios/axios`, `burntsushi/ripgrep`,
  `uutils/coreutils`;
- E118 held-out evaluation: `astral-sh/ruff`, `facebook/docusaurus`,
  `nushell/nushell`, `preactjs/preact`.

It also marked seven untouched repositories as preliminarily ineligible
because the draft scan found fewer than four usable instances:
`faker-ruby/faker`, `immutable-js/immutable-js`, `javaparser/javaparser`,
`jordansissel/fpm`, `mrdoob/three.js`, `nlohmann/json`, and
`reactivex/rxjava`.

That left this preliminary 21-repository pool:

- Java: `apache/druid`, `apache/lucene`, `google/gson`,
  `projectlombok/lombok`;
- Go: `caddyserver/caddy`, `gin-gonic/gin`, `gohugoio/hugo`,
  `hashicorp/terraform`;
- Ruby: `fastlane/fastlane`, `fluent/fluentd`, `jekyll/jekyll`,
  `rubocop/rubocop`;
- PHP: `briannesbitt/carbon`, `laravel/framework`,
  `php-cs-fixer/php-cs-fixer`, `phpoffice/phpspreadsheet`;
- C/C++: `fmtlib/fmt`, `jqlang/jq`, `micropython/micropython`,
  `redis/redis`, `valkey-io/valkey`.

These are preliminary design notes only. A future E118c preregistration must
independently re-audit eligibility, freeze repository roles and decision rules,
and receive separate authorization. No split, minimum, feature schema, model,
or statistical rule from the discarded draft is adopted here.
