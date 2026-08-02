// Record what the application asks the OS to open, and stop it opening.
//
// `tauri_plugin_opener` -> the `open` crate -> `/usr/bin/open -- <url>`, with
// an absolute path, so a shim earlier on PATH cannot intercept it. Interposing
// `posix_spawn` catches it at the syscall Rust's `Command::spawn` actually
// uses, which lets MATRIX AUT-001 and AUT-003 assert on the exact URL the app
// handed the system opener without ever raising a browser window or letting a
// real OAuth flow begin.
//
// Every spawn is logged to $WL_OPEN_LOG. `/usr/bin/open` is replaced with
// `/usr/bin/true` so the call still succeeds from the app's point of view;
// everything else is spawned unchanged.
//
// Build: clang -dynamiclib -o opener-shim.dylib opener_shim.c
// Use:   DYLD_INSERT_LIBRARIES=<dylib> WL_OPEN_LOG=<path> <app>

#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct interpose_s {
  void *replacement;
  void *original;
} interpose_t;

static void record(const char *path, char *const argv[]) {
  const char *out = getenv("WL_OPEN_LOG");
  if (!out) return;
  FILE *f = fopen(out, "a");
  if (!f) return;
  fprintf(f, "%s", path ? path : "(null)");
  for (int i = 0; argv && argv[i]; i++) fprintf(f, "\t%s", argv[i]);
  fprintf(f, "\n");
  fclose(f);
}

int wl_posix_spawn(pid_t *pid, const char *path,
                   const posix_spawn_file_actions_t *actions,
                   const posix_spawnattr_t *attrs, char *const argv[],
                   char *const envp[]) {
  record(path, argv);
  if (path && strcmp(path, "/usr/bin/open") == 0) {
    char *const stub[] = {"true", NULL};
    return posix_spawn(pid, "/usr/bin/true", actions, attrs, stub, envp);
  }
  return posix_spawn(pid, path, actions, attrs, argv, envp);
}

int wl_execve(const char *path, char *const argv[], char *const envp[]) {
  record(path, argv);
  if (path && strcmp(path, "/usr/bin/open") == 0) {
    char *const stub[] = {"true", NULL};
    return execve("/usr/bin/true", stub, envp);
  }
  return execve(path, argv, envp);
}

__attribute__((used)) static const interpose_t wl_interposers[]
    __attribute__((section("__DATA,__interpose"))) = {
        {(void *)wl_posix_spawn, (void *)posix_spawn},
        {(void *)wl_execve, (void *)execve},
};
