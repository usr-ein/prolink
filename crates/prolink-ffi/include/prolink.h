/* SPDX-License-Identifier: GPL-3.0-only
 *
 * The C ABI of the prolink library.
 *
 * Hand-written rather than generated, so it can carry the same explanations
 * the Rust side does. `layout.rs` asserts the size and alignment of every
 * struct here against the Rust definitions, so a field added on one side and
 * not the other fails the test suite rather than corrupting a caller's stack.
 *
 * Link against libprolink_ffi.a (static) or libprolink_ffi.dylib/.so.
 */

#ifndef PROLINK_H
#define PROLINK_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Bytes reserved for a device name: 20 on the wire, plus a NUL. */
#define PROLINK_NAME_LEN 24
/* Bytes reserved for an IPv4 address in dotted form, plus a NUL. */
#define PROLINK_ADDRESS_LEN 16

/* The result of a call. Negative values are failures. */
typedef enum {
    PROLINK_OK = 0,
    PROLINK_INVALID_ARGUMENT = -1,
    PROLINK_NO_INTERFACE = -2,
    PROLINK_BIND = -3,
    PROLINK_NO_DEVICE_NUMBER = -4,
    PROLINK_BAD_MEDIUM = -5,
    PROLINK_INTERNAL = -6,
    PROLINK_PANIC = -7
} ProlinkStatus;

typedef enum {
    PROLINK_DEVICE_UNKNOWN = 0,
    PROLINK_DEVICE_CDJ = 1,
    PROLINK_DEVICE_MIXER = 2,
    PROLINK_DEVICE_REKORDBOX = 3
} ProlinkDeviceKind;

typedef enum {
    PROLINK_SLOT_NONE = 0,
    PROLINK_SLOT_CD = 1,
    PROLINK_SLOT_SD = 2,
    PROLINK_SLOT_USB = 3,
    PROLINK_SLOT_REKORDBOX = 4
} ProlinkSlot;

typedef enum {
    PROLINK_PLAY_NO_TRACK = 0x00,
    PROLINK_PLAY_LOADING = 0x02,
    PROLINK_PLAY_PLAYING = 0x03,
    PROLINK_PLAY_LOOPING = 0x04,
    PROLINK_PLAY_PAUSED = 0x05,
    PROLINK_PLAY_CUED = 0x06,
    PROLINK_PLAY_CUE_PLAY = 0x07,
    PROLINK_PLAY_SEARCHING = 0x09,
    PROLINK_PLAY_SPUN_DOWN = 0x0e,
    PROLINK_PLAY_EMERGENCY = 0x12,
    PROLINK_PLAY_OTHER = 0xff
} ProlinkPlayState;

typedef enum {
    PROLINK_EVENT_DEVICE_FOUND = 1,
    PROLINK_EVENT_DEVICE_CHANGED = 2,
    PROLINK_EVENT_DEVICE_LOST = 3,
    PROLINK_EVENT_BEAT = 4,
    PROLINK_EVENT_PLAYER_CHANGED = 5,
    PROLINK_EVENT_TEMPO_MASTER = 6,
    PROLINK_EVENT_STOPPED = 7,
    PROLINK_EVENT_TRANSFER_PROGRESS = 8,
    PROLINK_EVENT_TRANSFER_DONE = 9
} ProlinkEventKind;

/* A device on the network. Strings are NUL-padded UTF-8 and never need
 * freeing: everything this API hands back is by value. */
typedef struct {
    uint8_t number;
    ProlinkDeviceKind kind;
    bool is_player;
    bool online;
    uint8_t mac[6];
    uint8_t name[PROLINK_NAME_LEN];
    uint8_t address[PROLINK_ADDRESS_LEN];
    uint64_t last_seen_ms;
} ProlinkDevice;

/* What one player is doing.
 *
 * The track, the play state and the tempo master need a status packet, which a
 * player unicasts only to peers that have announced themselves -- so they are
 * populated only when the session was opened with `announce`. `has_status`
 * says whether they mean anything. Absent numbers are negative, never zero, so
 * "not playing" is distinguishable from "0.00 BPM". */
typedef struct {
    uint8_t number;
    uint8_t name[PROLINK_NAME_LEN];
    bool has_status;
    bool is_beating;
    double effective_bpm;
    double track_bpm;
    double pitch_percent;
    double beat_phase;
    double bar_phase;
    uint8_t beat_in_bar;
    bool is_master;
    bool is_synced;
    uint8_t yielding_to;
    ProlinkPlayState play_state;
    uint32_t track_id;
    uint8_t track_source_player;
    ProlinkSlot track_source_slot;
} ProlinkPlayer;

typedef struct {
    uint8_t name[PROLINK_NAME_LEN];
    uint8_t address[PROLINK_ADDRESS_LEN];
    uint8_t broadcast[PROLINK_ADDRESS_LEN];
    bool is_link_local;
} ProlinkInterface;

/* One thing that happened. Switch on `kind` and read the fields it gives
 * meaning to; `dropped` is how many events were discarded before this one
 * because the host stopped polling, and non-zero means the incremental picture
 * is stale and the tables should be re-read. */
typedef struct {
    ProlinkEventKind kind;
    uint8_t device;
    uint8_t beat_in_bar;
    uint32_t dropped;
    uint32_t transfer;
    uint64_t done;
    uint64_t total;
    ProlinkStatus status;
} ProlinkEvent;

/* How to start a session. Zero `interface` to choose one automatically. */
typedef struct {
    uint8_t interface[PROLINK_NAME_LEN];
    bool announce;
    uint8_t device_number;
} ProlinkConfig;

typedef struct ProlinkSession ProlinkSession;

/* Static strings, valid for the life of the process. Do not free. */
const char* prolink_version(void);
/* The last error on this thread. Valid until the next call on this thread. */
const char* prolink_last_error(void);

ProlinkStatus prolink_config_default(ProlinkConfig* config);

int32_t prolink_interface_count(void);
int32_t prolink_interfaces(ProlinkInterface* out, int32_t capacity);

ProlinkStatus prolink_open(const ProlinkConfig* config, ProlinkSession** out);
/* Accepts NULL, so a host may close unconditionally. */
void prolink_close(ProlinkSession* session);

uint8_t prolink_device_number(const ProlinkSession* session);
int32_t prolink_devices(const ProlinkSession* session, ProlinkDevice* out, int32_t capacity);
int32_t prolink_players(const ProlinkSession* session, ProlinkPlayer* out, int32_t capacity);

/* Drain from the host's own event loop; nothing is ever pushed from a network
 * thread. Returns false when the queue is empty. */
bool prolink_next_event(const ProlinkSession* session, ProlinkEvent* out);

/* Fetch one file from a player's medium. Returns a transfer id, or a negative
 * ProlinkStatus. Progress arrives as PROLINK_EVENT_TRANSFER_PROGRESS carrying
 * that id, and exactly one PROLINK_EVENT_TRANSFER_DONE ends it.
 *
 * `remote_path` is taken verbatim from export.pdb, which stores paths relative
 * to the medium root with a leading slash. Nothing partial is ever written. */
int32_t prolink_fetch_file(const ProlinkSession* session,
        uint8_t device_number,
        ProlinkSlot slot,
        const char* remote_path,
        const char* local_path);

/* The same, for the database a browse is built from. */
int32_t prolink_fetch_database(const ProlinkSession* session,
        uint8_t device_number,
        ProlinkSlot slot,
        const char* local_path);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* PROLINK_H */
