# LightPost

Rust와 Axum, HTML, CSS, Vanilla JS로 만든 가벼운 개인 블로그의 첫 번째 수직 슬라이스입니다.

## 설계 원칙

- 공개 글은 일반 HTML 링크로 읽을 수 있고 JavaScript 없이도 동작합니다.
- JavaScript는 관리자 저장 같은 보조 상호작용에만 사용합니다.
- SQLite 파일 하나를 사용하며 초안은 공개 목록·검색·RSS·sitemap에서 제외합니다.
- 템플릿과 스타일은 작게 유지하고, 프론트엔드 번들러나 무거운 웹 프레임워크를 사용하지 않습니다.

## 실행

```powershell
.\run_webapp.ps1
```

서버는 `http://127.0.0.1:3000`에서 실행됩니다. 관리자 화면을 사용하려면 브라우저에서 `lightpost_session=local-secret` 쿠키를 설정해야 합니다. 운영 환경에서는 별도의 로그인 화면과 세션 저장소를 추가합니다.

옵션을 지정할 수도 있습니다.

```powershell
.\run_webapp.ps1 -Port 3100 -AdminToken "my-secret" -OpenBrowser
.\run_webapp.ps1 -Release
.\run_webapp.ps1 -DryRun
```

## 검증

```powershell
cargo test
```

현재 테스트는 Markdown 변환, slug 생성, 초안 비공개, 게시글 페이지, 관리자 세션 쿠키, RSS 공개 범위를 검증합니다.
