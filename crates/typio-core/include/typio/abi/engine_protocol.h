#ifndef TYPIO_ABI_ENGINE_PROTOCOL_H
#define TYPIO_ABI_ENGINE_PROTOCOL_H

#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define TYPIO_ENGINE_PROTOCOL_FD 3
#define TYPIO_ENGINE_PROTOCOL_MAGIC 0x54594550u /* TYEP */
#define TYPIO_ENGINE_PROTOCOL_MAJOR 1u
#define TYPIO_ENGINE_PROTOCOL_MINOR 0u
#define TYPIO_ENGINE_PROTOCOL_MAX_PAYLOAD (1u << 20)

typedef enum {
    TYPIO_ENGINE_PROTOCOL_ENGINE_HELLO = 1,
    TYPIO_ENGINE_PROTOCOL_HOST_HELLO = 2,
    TYPIO_ENGINE_PROTOCOL_REQUEST = 3,
    TYPIO_ENGINE_PROTOCOL_RESPONSE = 4,
    TYPIO_ENGINE_PROTOCOL_EVENT = 5,
    TYPIO_ENGINE_PROTOCOL_ERROR = 6,
} TypioEngineProtocolMessageType;

typedef struct {
    uint32_t message_type;
    uint32_t flags;
    uint64_t request_id;
    unsigned char *payload;
    size_t payload_len;
} TypioEngineProtocolFrame;

static inline uint16_t typio_engine_protocol_get_be16(const unsigned char *p) {
    return (uint16_t)(((uint16_t)p[0] << 8) | (uint16_t)p[1]);
}

static inline uint32_t typio_engine_protocol_get_be32(const unsigned char *p) {
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | (uint32_t)p[3];
}

static inline uint64_t typio_engine_protocol_get_be64(const unsigned char *p) {
    uint64_t hi = typio_engine_protocol_get_be32(p);
    uint64_t lo = typio_engine_protocol_get_be32(p + 4);
    return (hi << 32) | lo;
}

static inline void typio_engine_protocol_put_be16(unsigned char *p, uint16_t v) {
    p[0] = (unsigned char)(v >> 8);
    p[1] = (unsigned char)v;
}

static inline void typio_engine_protocol_put_be32(unsigned char *p, uint32_t v) {
    p[0] = (unsigned char)(v >> 24);
    p[1] = (unsigned char)(v >> 16);
    p[2] = (unsigned char)(v >> 8);
    p[3] = (unsigned char)v;
}

static inline void typio_engine_protocol_put_be64(unsigned char *p, uint64_t v) {
    typio_engine_protocol_put_be32(p, (uint32_t)(v >> 32));
    typio_engine_protocol_put_be32(p + 4, (uint32_t)v);
}

static inline bool typio_engine_protocol_read_exact(int fd, void *buf, size_t len) {
    unsigned char *p = (unsigned char *)buf;
    while (len > 0) {
        ssize_t n = read(fd, p, len);
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        if (n == 0) {
            return false;
        }
        p += (size_t)n;
        len -= (size_t)n;
    }
    return true;
}

static inline bool typio_engine_protocol_write_exact(int fd, const void *buf, size_t len) {
    const unsigned char *p = (const unsigned char *)buf;
    while (len > 0) {
        ssize_t n = write(fd, p, len);
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        p += (size_t)n;
        len -= (size_t)n;
    }
    return true;
}

static inline bool typio_engine_protocol_read_frame(int fd, TypioEngineProtocolFrame *frame) {
    unsigned char header[28];
    if (!frame) {
        return false;
    }
    memset(frame, 0, sizeof(*frame));
    if (!typio_engine_protocol_read_exact(fd, header, sizeof(header))) {
        return false;
    }
    if (typio_engine_protocol_get_be32(header) != TYPIO_ENGINE_PROTOCOL_MAGIC) {
        return false;
    }
    if (typio_engine_protocol_get_be16(header + 4) != TYPIO_ENGINE_PROTOCOL_MAJOR) {
        return false;
    }
    frame->message_type = typio_engine_protocol_get_be32(header + 8);
    frame->flags = typio_engine_protocol_get_be32(header + 12);
    frame->request_id = typio_engine_protocol_get_be64(header + 16);
    frame->payload_len = (size_t)typio_engine_protocol_get_be32(header + 24);
    if (frame->payload_len > TYPIO_ENGINE_PROTOCOL_MAX_PAYLOAD) {
        return false;
    }
    if (frame->payload_len == 0) {
        return true;
    }
    frame->payload = (unsigned char *)malloc(frame->payload_len + 1);
    if (!frame->payload) {
        return false;
    }
    if (!typio_engine_protocol_read_exact(fd, frame->payload, frame->payload_len)) {
        free(frame->payload);
        frame->payload = NULL;
        frame->payload_len = 0;
        return false;
    }
    frame->payload[frame->payload_len] = '\0';
    return true;
}

static inline bool typio_engine_protocol_write_frame(int fd,
                                                uint32_t message_type,
                                                uint64_t request_id,
                                                const void *payload,
                                                size_t payload_len) {
    unsigned char header[28];
    if (payload_len > TYPIO_ENGINE_PROTOCOL_MAX_PAYLOAD) {
        return false;
    }
    typio_engine_protocol_put_be32(header, TYPIO_ENGINE_PROTOCOL_MAGIC);
    typio_engine_protocol_put_be16(header + 4, TYPIO_ENGINE_PROTOCOL_MAJOR);
    typio_engine_protocol_put_be16(header + 6, TYPIO_ENGINE_PROTOCOL_MINOR);
    typio_engine_protocol_put_be32(header + 8, message_type);
    typio_engine_protocol_put_be32(header + 12, 0);
    typio_engine_protocol_put_be64(header + 16, request_id);
    typio_engine_protocol_put_be32(header + 24, (uint32_t)payload_len);
    if (!typio_engine_protocol_write_exact(fd, header, sizeof(header))) {
        return false;
    }
    return payload_len == 0 ||
           typio_engine_protocol_write_exact(fd, payload, payload_len);
}

static inline void typio_engine_protocol_frame_free(TypioEngineProtocolFrame *frame) {
    if (!frame) {
        return;
    }
    free(frame->payload);
    memset(frame, 0, sizeof(*frame));
}

static inline bool typio_engine_protocol_send_hello(int fd,
                                               const char *engine_name,
                                               const char *engine_type) {
    char payload[256];
    int n = snprintf(payload,
                     sizeof(payload),
                     "protocol\t1.0\nengine\t%s\ntype\t%s",
                     engine_name ? engine_name : "",
                     engine_type ? engine_type : "");
    if (n < 0 || (size_t)n >= sizeof(payload)) {
        return false;
    }
    return typio_engine_protocol_write_frame(fd,
                                        TYPIO_ENGINE_PROTOCOL_ENGINE_HELLO,
                                        0,
                                        payload,
                                        (size_t)n);
}

static inline bool typio_engine_protocol_send_response(int fd,
                                                  uint64_t request_id,
                                                  const char *payload,
                                                  size_t payload_len) {
    return typio_engine_protocol_write_frame(fd,
                                        TYPIO_ENGINE_PROTOCOL_RESPONSE,
                                        request_id,
                                        payload,
                                        payload_len);
}

static inline bool typio_engine_protocol_send_error(int fd,
                                               uint64_t request_id,
                                               const char *message) {
    const char *payload = message ? message : "engine protocol error";
    return typio_engine_protocol_write_frame(fd,
                                        TYPIO_ENGINE_PROTOCOL_ERROR,
                                        request_id,
                                        payload,
                                        strlen(payload));
}

#endif /* TYPIO_ABI_ENGINE_PROTOCOL_H */
