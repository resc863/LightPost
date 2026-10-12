# LightPost

Rust와 Axum, HTML, CSS, Vanilla JS로 만든 가벼운 개인 블로그입니다. 실행 파일 하나와 SQLite 파일 하나로 동작합니다.

## 설계 원칙

- 공개 글과 관리자 화면 모두 일반 HTML 링크와 폼으로 동작하며 JavaScript가 없어도 쓸 수 있습니다.
- JavaScript는 삭제 확인, 저장하지 않은 변경 경고 같은 보조 상호작용에만 사용합니다.
- SQLite 파일 하나를 사용하며 초안은 공개 목록·검색·태그·RSS·sitemap에서 제외합니다.
- CSS·JS·아이콘은 실행 파일에 내장되므로 어느 디렉터리에서 실행해도 됩니다.
- 의존성은 직접 만들면 위험한 부분(HTML 템플릿 escape, 난수)에만 추가합니다. 날짜 처리, 마이그레이션, 설정은 직접 구현했습니다.

## 실행

Windows (PowerShell):

```powershell
.\run_webapp.ps1
.\run_webapp.ps1 -Port 3100 -AdminToken "my-long-secret-token" -OpenBrowser
.\run_webapp.ps1 -Release
.\run_webapp.ps1 -DryRun
```

Linux / macOS:

```bash
./run_webapp.sh
./run_webapp.sh --port 3100 --token "my-long-secret-token" --release
./run_webapp.sh --dry-run
```

Docker:

```bash
docker build -t lightpost .
docker run -p 3000:3000 -v lightpost-data:/data \
  -e LIGHTPOST_ADMIN_TOKEN="my-long-secret-token" \
  -e LIGHTPOST_BASE_URL="https://blog.example.com" \
  lightpost
```

서버는 기본적으로 `http://127.0.0.1:3000`에서 실행됩니다. `/admin`에 접속해 관리자 토큰으로 로그인하면 글을 쓰고, 수정하고, 발행하고, 삭제할 수 있습니다. Ctrl+C나 SIGTERM을 받으면 처리 중인 요청을 마친 뒤 종료합니다.

## 설정

모든 설정은 환경 변수로 지정합니다. `LIGHTPOST_ADMIN_TOKEN`만 필수입니다.

| 변수 | 기본값 | 설명 |
|---|---|---|
| `LIGHTPOST_ADMIN_TOKEN` | (필수) | 관리자 로그인 토큰입니다. 16자 이상을 권장합니다. 비어 있으면 서버가 시작하지 않습니다. |
| `LIGHTPOST_HOST` | `127.0.0.1` | 바인드할 IP 주소입니다. 컨테이너에서는 `0.0.0.0`을 씁니다. |
| `LIGHTPOST_PORT` | `3000` | 포트입니다. |
| `LIGHTPOST_BASE_URL` | `http://{host}:{port}` | 외부에 공개되는 주소입니다. RSS·sitemap·canonical 링크에 쓰이며, `https://`로 시작하면 세션 쿠키에 `Secure`가 붙습니다. |
| `LIGHTPOST_DATA_DIR` | `data` | `blog.sqlite`를 저장할 디렉터리입니다. |
| `LIGHTPOST_UTC_OFFSET` | `+09:00` | 날짜를 표시할 시간대입니다. |
| `LIGHTPOST_PAGE_SIZE` | `10` | 목록 한 페이지의 글 수(1~100)입니다. |
| `LIGHTPOST_SITE_TITLE` | `LightPost` | 사이트 이름입니다. |
| `LIGHTPOST_SITE_TAGLINE` | `작은 기록이 쌓이는 곳.` | 푸터 문구입니다. |
| `LIGHTPOST_SITE_HEADLINE` | `작은 생각을 모아,\n오래 남을 기록으로.` | 홈 제목입니다. `\n`은 줄바꿈이 됩니다. |
| `LIGHTPOST_SITE_DESCRIPTION` | (소개 문구) | 홈 소개, meta description, RSS 설명에 쓰입니다. |
| `LIGHTPOST_SITE_LANG` | `ko` | `<html lang>`과 RSS `language` 값입니다. |

## 관리자 API

스크립트에서는 세션 대신 `Authorization: Bearer <토큰>` 헤더로 JSON API를 호출할 수 있습니다.

| 메서드 | 경로 | 설명 |
|---|---|---|
| `GET` | `/api/admin/posts` | 초안을 포함한 모든 글 목록 |
| `POST` | `/api/admin/posts` | 글 생성 (201) |
| `GET` | `/api/admin/posts/{id}` | 글 하나 |
| `PUT` | `/api/admin/posts/{id}` | 글 전체 수정 |
| `DELETE` | `/api/admin/posts/{id}` | 글 삭제 (204) |

```bash
curl -X POST http://127.0.0.1:3000/api/admin/posts \
  -H "Authorization: Bearer $LIGHTPOST_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"title": "첫 글", "body_markdown": "# 안녕", "tags": ["일상"], "status": "published"}'
```

`slug`, `excerpt`, `tags`, `status`(`draft` 기본값, `published`)는 생략할 수 있습니다. 잘못된 요청은 400, 같은 slug는 409로 응답합니다.

## 구조

```
src/
  config.rs     환경 변수 → Config
  domain.rs     Post, PostStatus, slug·태그 정규화, 입력 검증
  db/           SQLite 저장소(쓰기 연결 1개 + 읽기 연결 4개, WAL)와 user_version 마이그레이션
  markdown.rs   원본 HTML과 위험한 링크 스킴을 걸러내는 Markdown 렌더링
  render/       maud 템플릿 (공개 페이지, 관리자, RSS·sitemap)
  web/          라우터, 핸들러, 로그인·세션, 정적 파일, 미들웨어(보안 헤더, ETag, 접근 로그)
  time.rs       SQLite 시각 → 표시용·RFC 2822·RFC 3339 날짜
```

스키마를 바꿀 때는 `src/db/migrations.rs`의 `MIGRATIONS` 끝에 새 단계를 추가합니다. 이미 배포된 단계는 수정하지 않습니다. 첫 버전으로 만든 데이터베이스도 시작할 때 자동으로 최신 스키마로 옮겨집니다.

## 검증

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

테스트 범위는 [docs/TEST_PLAN.md](docs/TEST_PLAN.md)에 정리했습니다. GitHub Actions가 push와 pull request마다 같은 검사를 Linux·Windows·macOS에서 실행합니다.
