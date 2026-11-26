#include <os/log.h>
#include <os/signpost.h>
#include <stdint.h>
#include <sys/sysctl.h>
#include <sys/time.h>
#include <unistd.h>

enum {
    BRUK_LOG_LEVEL_DEFAULT = 0,
    BRUK_LOG_LEVEL_INFO = 1,
    BRUK_LOG_LEVEL_ERROR = 2,
};

os_log_t bruk_os_log_create(const char *subsystem, const char *category) {
    return os_log_create(subsystem, category);
}

void bruk_os_log_emit(os_log_t log, uint8_t level, const char *message) {
    if (log == NULL || message == NULL) {
        return;
    }
    os_log_type_t log_type = OS_LOG_TYPE_DEFAULT;
    switch (level) {
        case BRUK_LOG_LEVEL_INFO:
            log_type = OS_LOG_TYPE_INFO;
            break;
        case BRUK_LOG_LEVEL_ERROR:
            log_type = OS_LOG_TYPE_ERROR;
            break;
        case BRUK_LOG_LEVEL_DEFAULT:
        default:
            log_type = OS_LOG_TYPE_DEFAULT;
            break;
    }
    os_log_with_type(log, log_type, "%{public}s", message);
}

uint64_t bruk_os_signpost_begin(os_log_t log, const char *phase) {
    if (log == NULL || phase == NULL) {
        return 0;
    }
    if (__builtin_available(macOS 10.14, *)) {
        os_signpost_id_t spid = os_signpost_id_generate(log);
        os_signpost_interval_begin(log, spid, "boot_phase", "%{public}s", phase);
        return (uint64_t)spid;
    }
    return 0;
}

void bruk_os_signpost_end(os_log_t log, uint64_t spid, const char *phase, const char *detail) {
    if (log == NULL || spid == 0) {
        return;
    }
    if (__builtin_available(macOS 10.14, *)) {
        const char *safe_phase = phase != NULL ? phase : "";
        if (detail != NULL && detail[0] != '\0') {
            os_signpost_interval_end(
                log,
                (os_signpost_id_t)spid,
                "boot_phase",
                "%{public}s %{public}s",
                safe_phase,
                detail
            );
        } else {
            os_signpost_interval_end(
                log,
                (os_signpost_id_t)spid,
                "boot_phase",
                "%{public}s",
                safe_phase
            );
        }
    }
}

void bruk_os_signpost_event(os_log_t log, const char *name, const char *detail) {
    if (log == NULL || name == NULL) {
        return;
    }
    if (__builtin_available(macOS 10.14, *)) {
        if (detail != NULL && detail[0] != '\0') {
            os_signpost_event_emit(
                log,
                OS_SIGNPOST_ID_EXCLUSIVE,
                "boot_event",
                "%{public}s %{public}s",
                name,
                detail
            );
        } else {
            os_signpost_event_emit(
                log,
                OS_SIGNPOST_ID_EXCLUSIVE,
                "boot_event",
                "%{public}s",
                name
            );
        }
    }
}

double bruk_process_age_ms(void) {
    int mib[4] = { CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid() };
    struct kinfo_proc info;
    size_t size = sizeof(info);
    if (sysctl(mib, 4, &info, &size, NULL, 0) != 0 || size == 0) {
        return -1.0;
    }
    struct timeval now;
    if (gettimeofday(&now, NULL) != 0) {
        return -1.0;
    }
    double start_ms = (double)info.kp_proc.p_starttime.tv_sec * 1000.0
                    + (double)info.kp_proc.p_starttime.tv_usec / 1000.0;
    double now_ms = (double)now.tv_sec * 1000.0
                  + (double)now.tv_usec / 1000.0;
    double delta = now_ms - start_ms;
    return delta >= 0.0 ? delta : 0.0;
}
