# LightPost 테스트 계획

콘텐츠를 잃거나, 초안이 외부에 노출되거나, 관리자 권한이 탈취되는 문제가 가장 위험합니다. 그래서 저장소, 렌더링, HTTP 경계를 함께 검증합니다. 단위 테스트는 각 모듈의 `#[cfg(test)]`에, HTTP 테스트는 `tests/http.rs`에 있습니다.

## 단위 테스트

| ID | 영역 | 시나리오 | 기대 결과 |
|---|---|---|---|
| LP-UNIT-001 | Markdown | 제목과 강조가 포함된 Markdown을 변환 | `<h1>`과 `<strong>` HTML이 생성된다 |
| LP-UNIT-002 | Markdown | `<script>`, `<img onerror>` 같은 원본 HTML | 태그가 아닌 escape된 텍스트로 출력된다 |
| LP-UNIT-003 | Markdown | `javascript:`, 대소문자 섞인 스킴, 탭이 끼어든 스킴, `data:`, `vbscript:` 링크와 이미지 | 주소가 `#`으로 바뀐다 |
| LP-UNIT-004 | Markdown | http, https, mailto, 상대 경로, `#` 링크 | 그대로 유지된다 |
| LP-UNIT-005 | URL | 제목을 slug로 변환 | 공백·구두점이 제거되고, 글자가 없으면 빈 값이 된다 |
| LP-UNIT-006 | URL | `c#`, 공백, `/`, 한글을 경로 조각으로 인코딩 | RFC 3986 unreserved 외에는 percent-encoding된다 |
| LP-UNIT-007 | 태그 | 대소문자·공백·중복·쉼표·제어문자가 섞인 태그 | 정규화되고, 중복과 빈 값이 제거되며, 쉼표는 유지된다 |
| LP-UNIT-008 | 검증 | 알 수 없는 상태 값, slug를 만들 수 없는 제목 | `BadRequest`가 된다 |
| LP-UNIT-009 | 날짜 | SQLite 시각을 +09:00, UTC, -05:00으로 변환 | 표시 날짜·RFC 2822·RFC 3339가 지역 날짜 기준으로 맞다 |
| LP-UNIT-010 | 오류 | 내부 오류 응답 | 상세 메시지가 클라이언트에 노출되지 않는다 |
| LP-UNIT-011 | 인증 | 상수 시간 비교, 쿠키 파싱, Origin 검사, 세션 생성·삭제 | 같은 출처만 허용하고, 세션 ID는 무작위이며 폐기할 수 있다 |
| LP-UNIT-012 | 렌더링 | 사이트 이름·제목에 HTML 특수문자 | 자동으로 escape된다 |

## 저장소 테스트

| ID | 시나리오 | 기대 결과 |
|---|---|---|
| LP-STORE-001 | 발행 글과 초안을 함께 저장한 뒤 공개 조회 | 발행 글만 반환된다 |
| LP-STORE-002 | `rust`와 `rustacean`, `a,b` 태그로 조회 | 정확히 같은 태그만 반환된다 |
| LP-STORE-003 | `%`, `_`, 공백만 있는 검색어 | 와일드카드가 아닌 문자로 검색하고, 빈 검색어는 결과가 없다 |
| LP-STORE-004 | 초안 → 발행 → 수정 | `published_at`은 처음 발행할 때 한 번만 정해진다 |
| LP-STORE-005 | 글 삭제 | 아무 글도 쓰지 않는 태그가 함께 지워진다 |
| LP-STORE-006 | 같은 slug로 두 번 저장 | `Conflict`가 된다 |
| LP-STORE-007 | 글 5개를 2개씩 조회 | 페이지 수와 순서가 맞다 |
| LP-STORE-008 | 파일 DB에 저장한 뒤 다시 열기 | 글과 태그가 유지되고 스키마 버전이 최신이다 |
| LP-STORE-009 | 첫 버전 스키마(쉼표 태그, `user_version` 0)로 만든 DB 열기 | 태그 테이블로 옮겨지고, HTML 캐시와 `published_at`이 채워진다 |

## HTTP 테스트

| ID | 영역 | 시나리오 | 기대 결과 |
|---|---|---|---|
| LP-HTTP-001 | 글 상세 | 발행 글의 slug로 GET | 200, 제목·본문 HTML·절대 canonical URL |
| LP-HTTP-002 | 글 상세 | 초안의 slug, 없는 경로로 GET | 레이아웃을 갖춘 404 페이지 |
| LP-HTTP-003 | 관리자 | 세션 없이, 또는 토큰 자체를 쿠키로 넣고 `/admin` 접근 | 로그인 페이지로 303 |
| LP-HTTP-004 | 로그인 | 틀린 토큰, 맞는 토큰 | 401 / `HttpOnly; SameSite=Strict; Secure` 세션 쿠키, 쿠키에 토큰이 들어가지 않음 |
| LP-HTTP-005 | 로그아웃 | 로그아웃 후 같은 쿠키로 접근 | 세션이 폐기되어 로그인 페이지로 이동 |
| LP-HTTP-006 | CSRF | 다른 Origin에서 세션 쿠키로 POST | 403 |
| LP-HTTP-007 | 관리자 폼 | 초안 작성 → 발행 → 편집 화면 → 삭제 | 각 단계의 공개 여부와 리다이렉트가 맞다 |
| LP-HTTP-008 | 관리자 폼 | 잘못된 제목, 중복 slug | 400 / 409, 입력값과 오류 메시지가 유지된다 |
| LP-HTTP-009 | API | 인증 없이, 틀린 Bearer, 맞는 Bearer, 세션 쿠키 | 401 / 401 / 201 / 200 |
| LP-HTTP-010 | API | 깨진 JSON, 필드 누락, 빈 제목, `!!!` 제목, 알 수 없는 상태 | 모두 400 |
| LP-HTTP-011 | API | 같은 slug로 두 번 생성 | 409 |
| LP-HTTP-012 | API | 수정, 없는 글 수정, 삭제 | 200 / 404 / 204 |
| LP-HTTP-013 | XSS | 본문에 `<script>`, `javascript:` 링크 | 공개 페이지에 실행 가능한 형태로 나오지 않는다 |
| LP-HTTP-014 | 태그 | `c#`, `a b` 태그 링크와 태그 페이지, 없는 태그 | 인코딩된 링크, 정확한 결과, 404 |
| LP-HTTP-015 | 검색 | 같은 검색어의 발행 글과 초안, `%`, 빈 검색어 | 발행 글만, 문자 그대로, 결과 없음 |
| LP-HTTP-016 | 페이지 | 페이지당 2개로 글 3개 | 다음 페이지 링크, 마지막 페이지, 범위 밖은 404 |
| LP-HTTP-017 | RSS | 발행 글과 초안 | 발행 글만, 절대 URL, `guid`, `pubDate`(+0900), channel 필수 요소 |
| LP-HTTP-018 | Sitemap | 발행 글 | 절대 URL `loc`과 `lastmod` |
| LP-HTTP-019 | 캐시·보안 | 홈 GET, 같은 ETag로 재요청 | CSP·nosniff 헤더, 304와 빈 본문 |
| LP-HTTP-020 | 정적 파일 | 버전이 붙은 CSS, 없는 파일 | 내장 CSS와 `immutable` 캐시 / 404 |
| LP-HTTP-021 | 헬스체크 | `/healthz` | `ok` |

## 실행 명령

```bash
cargo test
```

SQLite의 `bundled` 빌드에는 C 컴파일러가 필요합니다. Windows에서 `cl.exe`가 등록돼 있지 않다면 `clang-cl`을 지정합니다.

```powershell
$env:CC = "clang-cl.exe"
cargo test
```

## 수동 확인

자동 테스트가 다루지 않는 부분은 브라우저에서 확인합니다.

- 로그인 → 새 글 작성(태그 `Rust, c#, 일상`, 본문에 `<script>` 포함) → 발행 → 홈·태그 페이지·글 페이지 → 삭제 확인 창 → 로그아웃
- 브라우저 콘솔에 CSP 위반이나 스크립트 오류가 없는지
- 모바일 폭(600px 이하)에서 목록, 편집기, 페이지 이동 링크의 배치
