// Loopback Linux fixture for recorder and journal synchronization failures.
#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <unistd.h>

static int fail_sync(int fd, const char *marker_name, const char *suffix) {
    const char *marker = getenv(marker_name);
    if (marker && access(marker, F_OK) == 0) {
        char link[64], path[PATH_MAX];
        snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
        ssize_t size = readlink(link, path, sizeof(path) - 1);
        size_t length = strlen(suffix);
        return size >= (ssize_t)length && memcmp(path + size - length, suffix, length) == 0;
    }
    return 0;
}

int fdatasync(int fd) {
    if (fail_sync(fd, "FAIL_SYNC_JSONL", ".jsonl")) {
        errno = EIO;
        return -1;
    }
    return syscall(SYS_fdatasync, fd);
}

int fsync(int fd) {
    if (fail_sync(fd, "FAIL_SYNC_WAL", ".wal")) {
        errno = EIO;
        return -1;
    }
    return syscall(SYS_fsync, fd);
}
