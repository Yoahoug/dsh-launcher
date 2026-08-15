/*
 * dsh-launcher 原生启动器(macOS .app 与 Windows .exe 共用同一份 C 源码)
 *
 * 职责(纯启动器定位):
 *   1. 单实例:探测 http://127.0.0.1:3090/api/health,已运行则只打开控制台(召回)
 *   2. 否则 detached 拉起 node <package>/apps/<current>/src/server.mjs
 *   3. 轮询健康(≤3s)→ 打开浏览器控制台 → 退出
 *
 * 包结构(版本化目录,更新时只切 CURRENT 指针,天然回滚):
 *   macOS: dsh-launcher.app/Contents/MacOS/dsh-launcher
 *          dsh-launcher.app/Contents/Resources/{launcher.json, apps/<ver>/{src,public,...}}
 *   Windows: dsh-launcher-windows-x64/{dsh-launcher.exe, launcher.json, apps/<ver>/{src,public,...}}
 *
 * 编译:
 *   macOS:  clang -O2 -arch arm64 -arch x86_64 -o dsh-launcher native/launcher.c
 *   Windows:cl /O2 /Fe:dsh-launcher.exe native/launcher.c /link /SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup shell32.lib ws2_32.lib user32.lib
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
  #define WIN32_LEAN_AND_MEAN
  #include <windows.h>
  #include <shellapi.h>
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #pragma comment(lib, "ws2_32.lib")
  #pragma comment(lib, "shell32.lib")
  #pragma comment(lib, "user32.lib")
  #define PATH_SEP '\\'
  #define PATH_LIST_SEP ';'
#else
  #include <unistd.h>
  #include <fcntl.h>
  #include <dirent.h>
  #include <sys/socket.h>
  #include <sys/stat.h>
  #include <sys/types.h>
  #include <netinet/in.h>
  #include <arpa/inet.h>
  #include <errno.h>
  #define PATH_SEP '/'
  #define PATH_LIST_SEP ':'
#endif

#define CONSOLE_URL "http://127.0.0.1:3090/"
#define HEALTH_URL  "/api/health"
#define LAUNCHER_PORT 3090

/* ── 平台小工具 ─────────────────────────────────────── */

static char *path_join(const char *a, const char *b) {
  size_t la = strlen(a), lb = strlen(b);
  char *out = (char *)malloc(la + lb + 2);
  memcpy(out, a, la);
  if (la && a[la - 1] != PATH_SEP) out[la++] = PATH_SEP;
  memcpy(out + la, b, lb + 1);
  return out;
}

/* 自身可执行文件目录 */
static int self_dir(char *buf, size_t bufsz) {
#if defined(_WIN32)
  wchar_t wbuf[2048];
  DWORD n = GetModuleFileNameW(NULL, wbuf, 2048);
  if (n == 0 || n >= 2048) return -1;
  /* 截到最后一个反斜杠 */
  for (DWORD i = n; i > 0; i--) {
    if (wbuf[i - 1] == L'\\') { wbuf[i - 1] = L'\0'; break; }
  }
  int len = WideCharToMultiByte(CP_UTF8, 0, wbuf, -1, buf, (int)bufsz, NULL, NULL);
  return len > 0 ? 0 : -1;
#else
  uint32_t sz = (uint32_t)bufsz;
  if (_NSGetExecutablePath(buf, &sz) != 0) return -1;
  for (size_t i = strlen(buf); i > 0; i--) {
    if (buf[i - 1] == '/') { buf[i - 1] = '\0'; break; }
  }
  return 0;
#endif
}

/* 路径存在性 */
static int file_exists(const char *path) {
#if defined(_WIN32)
  return GetFileAttributesA(path) != INVALID_FILE_ATTRIBUTES;
#else
  return access(path, F_OK) == 0;
#endif
}

/* 路径存在且可执行(node 候选) */
static int file_exec(const char *path) {
#if defined(_WIN32)
  return file_exists(path);
#else
  return access(path, X_OK) == 0;
#endif
}

/* 读取文件全部内容(小文件),失败返回 NULL */
static char *read_file(const char *path) {
  FILE *f = fopen(path, "rb");
  if (!f) return NULL;
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  if (n <= 0 || n > 1 << 20) { fclose(f); return NULL; }
  char *s = (char *)malloc((size_t)n + 1);
  if (fread(s, 1, (size_t)n, f) != (size_t)n) { fclose(f); free(s); return NULL; }
  s[n] = '\0';
  fclose(f);
  return s;
}

/* 从 launcher.json 提取 "current": "x.y.z" */
static char *parse_current(const char *json) {
  const char *key = strstr(json, "\"current\"");
  if (!key) return NULL;
  const char *q = strchr(key, ':');
  if (!q) return NULL;
  q = strchr(q, '"');
  if (!q) return NULL;
  q++;
  const char *end = strchr(q, '"');
  if (!end) return NULL;
  size_t len = (size_t)(end - q);
  char *ver = (char *)malloc(len + 1);
  memcpy(ver, q, len);
  ver[len] = '\0';
  return ver;
}

/* ── node 发现 ──────────────────────────────────────── */

static char *xstrdup(const char *s) {
  size_t n = strlen(s);
  char *d = (char *)malloc(n + 1);
  memcpy(d, s, n + 1);
  return d;
}

static char *node_from_path(void) {
  const char *path = getenv("PATH");
  if (!path) return NULL;
  char *dup = xstrdup(path);
  char *cur = dup;
  while (*cur) {
    char *sep = strchr(cur, PATH_LIST_SEP);
    if (sep) *sep = '\0';
    if (*cur) {
      char *cand = path_join(cur, "node");
#if defined(_WIN32)
      char *cand2 = path_join(cur, "node.exe");
#else
      char *cand2 = xstrdup(cand);
#endif
      if (file_exec(cand2)) { free(dup); free(cand); return cand2; }
      free(cand);
      free(cand2);
    }
    if (!sep) break;
    cur = sep + 1;
  }
  free(dup);
  return NULL;
}

static char *node_discover(void) {
  const char *ov = getenv("DSH_NODE_BIN");
  if (ov && file_exec(ov)) return xstrdup(ov);
  char *p = node_from_path();
  if (p) return p;
  const char *cands[] = {
    "/opt/homebrew/bin/node", "/usr/local/bin/node", "/opt/local/bin/node", NULL
  };
  for (int i = 0; cands[i]; i++) {
    if (file_exec(cands[i])) return xstrdup(cands[i]);
  }
#if defined(_WIN32)
  const char *pf = getenv("ProgramFiles");
  const char *lpf = getenv("LOCALAPPDATA");
  char fixed[1024];
  if (pf) {
    snprintf(fixed, sizeof(fixed), "%s\\nodejs\\node.exe", pf);
    if (file_exec(fixed)) return xstrdup(fixed);
  }
  if (lpf) {
    snprintf(fixed, sizeof(fixed), "%s\\Programs\\nodejs\\node.exe", lpf);
    if (file_exec(fixed)) return xstrdup(fixed);
  }
#else
  /* nvm 目录扫描(macOS/Linux) */
  {
    const char *home = getenv("HOME");
    if (home) {
      char nvmdir[1024];
      snprintf(nvmdir, sizeof(nvmdir), "%s/.nvm/versions/node", home);
      DIR *d = opendir(nvmdir);
      if (d) {
        struct dirent *e;
        char best[1024] = "";
        unsigned bestv[3] = {0, 0, 0};
        while ((e = readdir(d)) != NULL) {
          if (e->d_name[0] == '.') continue;
          unsigned v[3] = {0, 0, 0};
          if (sscanf(e->d_name, "v%u.%u.%u", &v[0], &v[1], &v[2]) != 3) continue;
          if (v[0] > bestv[0] || (v[0] == bestv[0] && v[1] > bestv[1]) ||
              (v[0] == bestv[0] && v[1] == bestv[1] && v[2] > bestv[2])) {
            bestv[0] = v[0]; bestv[1] = v[1]; bestv[2] = v[2];
            snprintf(best, sizeof(best), "%s/%s/bin/node", nvmdir, e->d_name);
          }
        }
        closedir(d);
        if (best[0] && file_exec(best)) return xstrdup(best);
      }
    }
  }
#endif
  return NULL;
}

/* ── 健康探测(裸 socket GET)────────────────────────── */

static int probe_health(void) {
#if defined(_WIN32)
  WSADATA wsa;
  if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) return 0;
  SOCKET s = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
  if (s == INVALID_SOCKET) { WSACleanup(); return 0; }
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(LAUNCHER_PORT);
  addr.sin_addr.s_addr = inet_addr("127.0.0.1");
  int ok = connect(s, (struct sockaddr *)&addr, sizeof(addr)) == 0;
  if (ok) {
    const char *req = "GET " HEALTH_URL " HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n";
    send(s, req, (int)strlen(req), 0);
    char buf[64];
    int r = recv(s, buf, sizeof(buf), 0);
    ok = r > 0;
  }
  closesocket(s);
  WSACleanup();
  return ok;
#else
  int s = socket(AF_INET, SOCK_STREAM, 0);
  if (s < 0) return 0;
  struct sockaddr_in addr;
  memset(&addr, 0, sizeof(addr));
  addr.sin_family = AF_INET;
  addr.sin_port = htons(LAUNCHER_PORT);
  addr.sin_addr.s_addr = inet_addr("127.0.0.1");
  struct timeval tv = { 1, 0 };
  setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
  int ok = connect(s, (struct sockaddr *)&addr, sizeof(addr)) == 0;
  if (ok) {
    const char *req = "GET " HEALTH_URL " HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n";
    (void)!write(s, req, strlen(req));
    char buf[64];
    ok = read(s, buf, sizeof(buf)) > 0;
  }
  close(s);
  return ok;
#endif
}

/* ── 打开浏览器 ─────────────────────────────────────── */

static void open_browser(const char *url) {
  const char *no = getenv("DSH_NO_AUTOOPEN");
  if (no && no[0] != '\0' && no[0] != '0') {
    printf("控制台就绪:%s\n", url);
    fflush(stdout);
    return;
  }
#if defined(_WIN32)
  wchar_t wurl[1024];
  MultiByteToWideChar(CP_UTF8, 0, url, -1, wurl, 1024);
  ShellExecuteW(NULL, L"open", wurl, NULL, NULL, SW_SHOWNORMAL);
#else
  pid_t pid = fork();
  if (pid == 0) {
    execlp("open", "open", url, (char *)NULL);
    _exit(127);
  }
#endif
}

/* ── detached 拉起 node server ──────────────────────── */

static int mkdir_p(const char *path) {
  char tmp[2048];
  snprintf(tmp, sizeof(tmp), "%s", path);
  size_t len = strlen(tmp);
  if (len == 0) return -1;
  if (tmp[len - 1] == PATH_SEP) tmp[len - 1] = '\0';
  for (char *p = tmp + 1; *p; p++) {
    if (*p == PATH_SEP) {
      *p = '\0';
#if defined(_WIN32)
      CreateDirectoryA(tmp, NULL);
#else
      mkdir(tmp, 0755);
#endif
      *p = PATH_SEP;
    }
  }
#if defined(_WIN32)
  return CreateDirectoryA(tmp, NULL) ? 0 : (GetLastError() == ERROR_ALREADY_EXISTS ? 0 : -1);
#else
  return mkdir(tmp, 0755);
#endif
}

static void spawn_server(const char *node, const char *server_path, const char *workdir) {
  char logfile[2048];
  const char *home = getenv("HOME");
#if defined(_WIN32)
  const char *up = getenv("USERPROFILE");
  home = (home && home[0]) ? home : (up ? up : ".");
#endif
  snprintf(logfile, sizeof(logfile), "%s/.local/state/dsh-launcher/logs/launcher.out.log", home ? home : ".");
  mkdir_p(logfile);
#if defined(_WIN32)
  STARTUPINFOW si;
  PROCESS_INFORMATION pi;
  memset(&si, 0, sizeof(si));
  memset(&pi, 0, sizeof(pi));
  si.cb = sizeof(si);
  si.dwFlags = STARTF_USESTDHANDLES;
  HANDLE hlog = CreateFileA(logfile, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                            NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
  si.hStdOutput = hlog != INVALID_HANDLE_VALUE ? hlog : GetStdHandle(STD_OUTPUT_HANDLE);
  si.hStdError = si.hStdOutput;
  si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
  wchar_t wcmd[4096], wcwd[2048];
  /* 命令:node <server_path>(node 带引号,路径可能含空格) */
  {
    char cmd[4096];
    snprintf(cmd, sizeof(cmd), "\"%s\" \"%s\"", node, server_path);
    MultiByteToWideChar(CP_UTF8, 0, cmd, -1, wcmd, 4096);
  }
  MultiByteToWideChar(CP_UTF8, 0, workdir, -1, wcwd, 2048);
  if (CreateProcessW(NULL, wcmd, NULL, NULL, TRUE,
                     DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP,
                     NULL, wcwd, &si, &pi)) {
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
  }
  if (hlog != INVALID_HANDLE_VALUE) CloseHandle(hlog);
#else
  pid_t pid = fork();
  if (pid == 0) {
    setsid();
    int fd = open(logfile, O_WRONLY | O_CREAT | O_APPEND, 0644);
    if (fd >= 0) {
      dup2(fd, 1);
      dup2(fd, 2);
    } else {
      int nul = open("/dev/null", O_WRONLY);
      if (nul >= 0) { dup2(nul, 1); dup2(nul, 2); }
    }
    chdir(workdir);
    execl(node, node, server_path, (char *)NULL);
    _exit(127);
  }
#endif
}

/* ── 主流程 ─────────────────────────────────────────── */

int main(void) {
  char exedir[2048];
  if (self_dir(exedir, sizeof(exedir)) != 0) return 1;

  /* 定位包根与版本目录 */
  char root[2048];
#if defined(_WIN32)
  snprintf(root, sizeof(root), "%s", exedir);
#else
  snprintf(root, sizeof(root), "%s/../Resources", exedir);
#endif
  char *lj = path_join(root, "launcher.json");
  char *json = read_file(lj);
  char *ver = json ? parse_current(json) : NULL;
  free(lj);
  if (!ver) {
    fprintf(stderr, "dsh-launcher: 缺少 launcher.json(current 版本指针),包结构无效:%s\n", root);
    return 1;
  }
  char *appdir = path_join(root, "apps");
  char *verdir = path_join(appdir, ver);
  char *server_path = path_join(verdir, "src/server.mjs");

  if (!file_exists(server_path)) {
    fprintf(stderr, "dsh-launcher: 找不到 %s,包结构无效\n", server_path);
    return 1;
  }

  /* 单实例:已运行 → 召回 */
  if (probe_health()) {
    open_browser(CONSOLE_URL);
    return 0;
  }

  /* 找 node */
  char *node = node_discover();
  if (!node) {
    fprintf(stderr, "dsh-launcher: 未找到 Node.js(需要 ^22.19 || >=24),请先安装后重试\n");
    return 2;
  }

  spawn_server(node, server_path, verdir);
  free(node);

  /* 等待就绪(≤3s) */
  for (int i = 0; i < 30; i++) {
#ifdef _WIN32
    Sleep(100);
#else
    usleep(100 * 1000);
#endif
    if (probe_health()) {
      open_browser(CONSOLE_URL);
      return 0;
    }
  }

  fprintf(stderr, "dsh-launcher: 服务启动超时,请查看日志 ~/.local/state/dsh-launcher/logs/\n");
  return 3;
}
