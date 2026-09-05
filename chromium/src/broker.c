// SPDX-License-Identifier: GPL-3.0-only
#define _XOPEN_SOURCE 700

#include <errno.h>
#include <fcntl.h>
#include <gio/gio.h>
#include <glib.h>
#include <json-glib/json-glib.h>
#include <stdio.h>
#include <string.h>
#include <sys/file.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#include <ftw.h>

#define BUS_NAME "io.github.cheviiot.bastle.Chromium"
#define OBJECT_PATH "/io/github/cheviiot/bastle/Chromium/Engine1"
#define INTERFACE_NAME "io.github.cheviiot.bastle.Chromium.Engine1"
#define PROTOCOL_VERSION 1U
#define MAX_POLICY_SIZE (32U * 1024U * 1024U)

static const gchar introspection_xml[] =
    "<node>"
    " <interface name='" INTERFACE_NAME "'>"
    "  <method name='GetCapabilities'>"
    "   <arg name='protocol_version' type='u' direction='out'/>"
    "   <arg name='features' type='as' direction='out'/>"
    "  </method>"
    "  <method name='OpenApp'>"
    "   <arg name='id' type='s' direction='in'/>"
    "   <arg name='url' type='s' direction='in'/>"
    "   <arg name='title' type='s' direction='in'/>"
    "   <arg name='user_agent' type='s' direction='in'/>"
    "   <arg name='width' type='i' direction='in'/>"
    "   <arg name='height' type='i' direction='in'/>"
    "   <arg name='maximized' type='b' direction='in'/>"
    "   <arg name='start_in_background' type='b' direction='in'/>"
    "   <arg name='token' type='s' direction='in'/>"
    "   <arg name='policy_json' type='s' direction='in'/>"
    "  </method>"
    "  <method name='DeleteProfile'>"
    "   <arg name='id' type='s' direction='in'/>"
    "   <arg name='token' type='s' direction='in'/>"
    "  </method>"
    " </interface>"
    "</node>";

static GMainLoop *main_loop;

typedef struct {
    GSubprocess *child;
    int lock_fd;
    gchar *id;
    guint bridge_source_id;
    guint bridge_attempts;
} RuntimeProcess;

static gboolean
valid_app_id(const gchar *value)
{
    if (value == NULL || strlen(value) != 12)
        return FALSE;
    for (const guchar *cursor = (const guchar *) value; *cursor; cursor++)
        if (!(g_ascii_islower(*cursor) || g_ascii_isdigit(*cursor)))
            return FALSE;
    return TRUE;
}

static gboolean
valid_token(const gchar *value)
{
    if (value == NULL || strlen(value) != 64)
        return FALSE;
    for (const guchar *cursor = (const guchar *) value; *cursor; cursor++)
        if (!(g_ascii_isdigit(*cursor) || (*cursor >= 'a' && *cursor <= 'f')))
            return FALSE;
    return TRUE;
}

static gboolean
constant_time_equal(const gchar *left, const gchar *right)
{
    guchar different = 0;
    for (gsize index = 0; index < 64; index++)
        different |= (guchar) left[index] ^ (guchar) right[index];
    return different == 0;
}

static gboolean
valid_http_url(const gchar *value)
{
    g_autoptr(GError) error = NULL;
    g_autoptr(GUri) uri = g_uri_parse(value, G_URI_FLAGS_NONE, &error);
    if (uri == NULL || g_uri_get_host(uri) == NULL)
        return FALSE;
    const gchar *scheme = g_uri_get_scheme(uri);
    if (scheme == NULL)
        return FALSE;
    return g_ascii_strcasecmp(scheme, "http") == 0 ||
           g_ascii_strcasecmp(scheme, "https") == 0;
}

static gboolean
valid_title(const gchar *value)
{
    if (value == NULL || *value == '\0' || !g_utf8_validate(value, -1, NULL) ||
        g_utf8_strlen(value, -1) > 512)
        return FALSE;
    for (const gchar *cursor = value; *cursor; cursor = g_utf8_next_char(cursor))
        if (g_unichar_iscntrl(g_utf8_get_char(cursor)))
            return FALSE;
    return TRUE;
}

static gboolean
valid_user_agent(const gchar *value)
{
    return value != NULL && strlen(value) <= 4096 &&
           g_utf8_validate(value, -1, NULL) && strchr(value, '\n') == NULL &&
           strchr(value, '\r') == NULL;
}

static gboolean
valid_policy(const gchar *value, JsonNode **root_out, GError **error)
{
    if (value == NULL || strlen(value) > MAX_POLICY_SIZE) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "policy exceeds the protocol size limit");
        return FALSE;
    }
    g_autoptr(JsonParser) parser = json_parser_new();
    if (!json_parser_load_from_data(parser, value, -1, error))
        return FALSE;
    JsonNode *root = json_parser_get_root(parser);
    if (root == NULL || !JSON_NODE_HOLDS_OBJECT(root)) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "policy must be a JSON object");
        return FALSE;
    }
    JsonObject *object = json_node_get_object(root);
    if (!json_object_has_member(object, "schema_version") ||
        json_object_get_int_member(object, "schema_version") != 2) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_NOT_SUPPORTED,
                            "unsupported Bastle policy version");
        return FALSE;
    }
    *root_out = json_node_copy(root);
    return TRUE;
}

static gchar *
data_root(void)
{
    return g_build_filename(g_get_user_data_dir(), "bastle-chromium", NULL);
}

static gchar *
profile_path(const gchar *id)
{
    g_autofree gchar *root = data_root();
    return g_build_filename(root, "profiles", id, NULL);
}

static gchar *
cache_path(const gchar *id)
{
    return g_build_filename(g_get_user_cache_dir(), "bastle-chromium", id, NULL);
}

static gchar *
token_path(const gchar *id)
{
    g_autofree gchar *root = data_root();
    return g_build_filename(root, "tokens", id, NULL);
}

static gchar *
profile_lock_path(const gchar *id)
{
    g_autofree gchar *root = data_root();
    return g_build_filename(root, "locks", id, NULL);
}

static gboolean
write_all(int fd, const gchar *contents, gsize length, GError **error)
{
    while (length > 0) {
        ssize_t written = write(fd, contents, length);
        if (written < 0) {
            if (errno == EINTR)
                continue;
            g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                        "write failed: %s", g_strerror(errno));
            return FALSE;
        }
        contents += written;
        length -= (gsize) written;
    }
    return TRUE;
}

static gboolean
ensure_token(const gchar *id, const gchar *token, gboolean create, GError **error)
{
    if (!valid_token(token)) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "invalid app capability token");
        return FALSE;
    }
    g_autofree gchar *path = token_path(id);
    g_autofree gchar *directory = g_path_get_dirname(path);
    if (g_mkdir_with_parents(directory, 0700) != 0) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot create token directory: %s", g_strerror(errno));
        return FALSE;
    }

    g_autofree gchar *stored = NULL;
    gsize stored_length = 0;
    if (g_file_get_contents(path, &stored, &stored_length, NULL)) {
        if (stored_length != 64 || !valid_token(stored) ||
            !constant_time_equal(stored, token)) {
            g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_PERMISSION_DENIED,
                                "app capability token does not match");
            return FALSE;
        }
        return TRUE;
    }
    if (!create) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_NOT_FOUND,
                            "app capability token is not registered");
        return FALSE;
    }

    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) {
        if (errno == EEXIST)
            return ensure_token(id, token, FALSE, error);
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot create app token: %s", g_strerror(errno));
        return FALSE;
    }
    gboolean success = write_all(fd, token, 64, error) && fsync(fd) == 0;
    if (close(fd) != 0 && success) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot close app token: %s", g_strerror(errno));
        success = FALSE;
    }
    if (!success)
        unlink(path);
    return success;
}

static gchar *
write_runtime_config(const gchar *id, const gchar *url, const gchar *title,
                     const gchar *user_agent, gint width, gint height,
                     gboolean maximized, gboolean start_in_background,
                     JsonNode *policy, GError **error)
{
    g_autofree gchar *runtime_dir =
        g_build_filename(g_get_user_runtime_dir(), "bastle-chromium", NULL);
    if (g_mkdir_with_parents(runtime_dir, 0700) != 0) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot create runtime directory: %s", g_strerror(errno));
        return NULL;
    }
    g_autofree gchar *template =
        g_build_filename(runtime_dir, "request-XXXXXX.json", NULL);
    int fd = g_mkstemp_full(template, O_RDWR | O_CLOEXEC, 0600);
    if (fd < 0) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot create runtime request: %s", g_strerror(errno));
        return NULL;
    }

    g_autoptr(JsonBuilder) builder = json_builder_new();
    json_builder_begin_object(builder);
#define ADD_STRING(member, value) do { \
    json_builder_set_member_name(builder, member); \
    json_builder_add_string_value(builder, value); \
} while (0)
    json_builder_set_member_name(builder, "schema_version");
    json_builder_add_int_value(builder, 1);
    ADD_STRING("id", id);
    ADD_STRING("url", url);
    ADD_STRING("title", title);
    ADD_STRING("user_agent", user_agent);
    json_builder_set_member_name(builder, "width");
    json_builder_add_int_value(builder, width);
    json_builder_set_member_name(builder, "height");
    json_builder_add_int_value(builder, height);
    json_builder_set_member_name(builder, "maximized");
    json_builder_add_boolean_value(builder, maximized);
    json_builder_set_member_name(builder, "start_in_background");
    json_builder_add_boolean_value(builder, start_in_background);
    json_builder_set_member_name(builder, "policy");
    json_builder_add_value(builder, json_node_copy(policy));
    json_builder_end_object(builder);
#undef ADD_STRING
    g_autoptr(JsonNode) root = json_builder_get_root(builder);
    g_autoptr(JsonGenerator) generator = json_generator_new();
    json_generator_set_root(generator, root);
    gsize length = 0;
    g_autofree gchar *contents = json_generator_to_data(generator, &length);
    gboolean success = write_all(fd, contents, length, error) && fsync(fd) == 0;
    if (close(fd) != 0 && success)
        success = FALSE;
    if (!success) {
        unlink(template);
        if (*error == NULL)
            g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                        "cannot commit runtime request: %s", g_strerror(errno));
        return NULL;
    }
    return g_steal_pointer(&template);
}

static gboolean
open_profile_lock(const gchar *id, int operation, int *fd_out, GError **error)
{
    g_autofree gchar *lock_path = profile_lock_path(id);
    g_autofree gchar *lock_directory = g_path_get_dirname(lock_path);
    if (g_mkdir_with_parents(lock_directory, 0700) != 0) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot create Chromium lock directory: %s",
                    g_strerror(errno));
        return FALSE;
    }
    int lock_fd = open(lock_path, O_RDWR | O_CREAT, 0600);
    if (lock_fd < 0 || flock(lock_fd, operation | LOCK_NB) != 0) {
        if (lock_fd >= 0)
            close(lock_fd);
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_BUSY,
                            "Chromium profile is in use");
        return FALSE;
    }
    *fd_out = lock_fd;
    return TRUE;
}

static gchar *
runtime_socket_path(const gchar *id)
{
    g_autofree gchar *runtime_dir =
        g_build_filename(g_get_user_runtime_dir(), "bastle-chromium", NULL);
    g_autofree gchar *filename = g_strdup_printf("%s.sock", id);
    return g_build_filename(runtime_dir, filename, NULL);
}

static gboolean
runtime_socket_active(const gchar *id)
{
    g_autofree gchar *socket_path = runtime_socket_path(id);
    if (strlen(socket_path) >= sizeof(((struct sockaddr_un *) 0)->sun_path))
        return FALSE;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return FALSE;
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    g_strlcpy(address.sun_path, socket_path, sizeof(address.sun_path));
    gboolean active = connect(fd, (struct sockaddr *) &address,
                              sizeof(address)) == 0;
    close(fd);
    return active;
}

static void
free_runtime_process(RuntimeProcess *runtime)
{
    if (runtime->lock_fd >= 0)
        close(runtime->lock_fd);
    g_clear_object(&runtime->child);
    g_free(runtime->id);
    g_free(runtime);
}

static gboolean
runtime_lock_bridge_tick(gpointer user_data)
{
    RuntimeProcess *runtime = user_data;
    runtime->bridge_attempts++;
    if (!runtime_socket_active(runtime->id) &&
        runtime->bridge_attempts < 100)
        return G_SOURCE_CONTINUE;

    if (runtime->lock_fd >= 0) {
        close(runtime->lock_fd);
        runtime->lock_fd = -1;
    }
    runtime->bridge_source_id = 0;
    if (runtime->child == NULL)
        free_runtime_process(runtime);
    return G_SOURCE_REMOVE;
}

static void
runtime_exited(GObject *source, GAsyncResult *result, gpointer user_data)
{
    (void) source;
    RuntimeProcess *runtime = user_data;
    g_autoptr(GError) error = NULL;
    if (!g_subprocess_wait_finish(runtime->child, result, &error))
        g_warning("Failed to reap Chromium runtime: %s", error->message);
    g_object_unref(runtime->child);
    runtime->child = NULL;
    if (runtime->bridge_source_id == 0)
        free_runtime_process(runtime);
}

static gboolean
spawn_runtime(const gchar *id, const gchar *config_path, int lock_fd,
              GError **error)
{
    GSubprocess *child = g_subprocess_new(
        G_SUBPROCESS_FLAGS_STDOUT_SILENCE,
        error, "/app/bin/bastle-chromium-service", "--runtime", id,
        config_path, NULL);
    if (child == NULL)
        return FALSE;
    RuntimeProcess *runtime = g_new0(RuntimeProcess, 1);
    runtime->child = child;
    runtime->lock_fd = lock_fd;
    runtime->id = g_strdup(id);
    runtime->bridge_source_id = g_timeout_add_full(
        G_PRIORITY_DEFAULT, 50, runtime_lock_bridge_tick, runtime, NULL);
    g_subprocess_wait_async(child, NULL, runtime_exited, runtime);
    return TRUE;
}

static int
remove_entry(const char *path, const struct stat *status, int type, struct FTW *walk)
{
    (void) status;
    (void) walk;
    return type == FTW_DP ? rmdir(path) : unlink(path);
}

static gboolean
remove_tree(const gchar *path, GError **error)
{
    if (!g_file_test(path, G_FILE_TEST_EXISTS))
        return TRUE;
    if (nftw(path, remove_entry, 32, FTW_DEPTH | FTW_PHYS) != 0) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot remove profile data: %s", g_strerror(errno));
        return FALSE;
    }
    return TRUE;
}

static gboolean
delete_profile(const gchar *id, const gchar *token, GError **error)
{
    g_autofree gchar *profile = profile_path(id);
    g_autofree gchar *cache = cache_path(id);
    g_autofree gchar *token_file = token_path(id);
    int lock_fd = -1;
    if (!open_profile_lock(id, LOCK_EX, &lock_fd, error))
        return FALSE;

    gboolean success = FALSE;
    if (!g_file_test(token_file, G_FILE_TEST_EXISTS)) {
        if (!g_file_test(profile, G_FILE_TEST_EXISTS) &&
            !g_file_test(cache, G_FILE_TEST_EXISTS)) {
            success = TRUE;
            goto out;
        }
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_PERMISSION_DENIED,
                            "unowned Chromium profile cannot be removed");
        goto out;
    }
    if (!ensure_token(id, token, FALSE, error))
        goto out;

    if (g_file_test(profile, G_FILE_TEST_EXISTS)) {
        if (runtime_socket_active(id)) {
            g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_BUSY,
                                "Chromium profile is in use");
            goto out;
        }
        g_autofree gchar *socket_path = runtime_socket_path(id);
        if (unlink(socket_path) != 0 && errno != ENOENT) {
            g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                        "cannot remove stale runtime socket: %s",
                        g_strerror(errno));
            goto out;
        }
    }
    success = remove_tree(profile, error) && remove_tree(cache, error);
    if (success && unlink(token_file) != 0 && errno != ENOENT) {
        g_set_error(error, G_IO_ERROR, g_io_error_from_errno(errno),
                    "cannot remove profile token: %s", g_strerror(errno));
        success = FALSE;
    }

out:
    close(lock_fd);
    return success;
}

static void
return_error(GDBusMethodInvocation *invocation, GError *error)
{
    g_dbus_method_invocation_return_dbus_error(
        invocation, "io.github.cheviiot.bastle.Chromium.Error",
        error != NULL ? error->message : "unknown Chromium engine error");
}

static void
handle_method_call(GDBusConnection *connection, const gchar *sender,
                   const gchar *object_path, const gchar *interface_name,
                   const gchar *method_name, GVariant *parameters,
                   GDBusMethodInvocation *invocation, gpointer user_data)
{
    (void) object_path;
    (void) interface_name;
    (void) user_data;
    (void) connection;
    (void) sender;
    g_autoptr(GError) error = NULL;
    if (g_str_equal(method_name, "GetCapabilities")) {
        const gchar *features[] = {
            "open-app", "policy-v2", "profile-delete", "permissions",
            "navigation-allowlist", "proxy", "background",
            "download-dialog", "oauth-popups", NULL
        };
        g_dbus_method_invocation_return_value(
            invocation,
            g_variant_new("(u@as)", PROTOCOL_VERSION,
                          g_variant_new_strv(features, -1)));
        return;
    }
    if (g_str_equal(method_name, "OpenApp")) {
        const gchar *id, *url, *title, *user_agent, *token, *policy_json;
        gint width, height;
        gboolean maximized, start_in_background;
        g_variant_get(parameters, "(&s&s&s&siibb&s&s)", &id, &url, &title,
                      &user_agent, &width, &height, &maximized,
                      &start_in_background, &token, &policy_json);
        g_autoptr(JsonNode) policy = NULL;
        if (!valid_app_id(id) || !valid_http_url(url) || !valid_title(title) ||
            !valid_user_agent(user_agent) || width < 320 || width > 8192 ||
            height < 200 || height > 8192 ||
            !valid_policy(policy_json, &policy, &error)) {
            if (error == NULL)
                g_set_error_literal(&error, G_IO_ERROR,
                                    G_IO_ERROR_INVALID_ARGUMENT,
                                    "invalid OpenApp request");
            return_error(invocation, error);
            return;
        }
        int lock_fd = -1;
        if (!open_profile_lock(id, LOCK_SH, &lock_fd, &error)) {
            return_error(invocation, error);
            return;
        }
        if (!ensure_token(id, token, TRUE, &error)) {
            close(lock_fd);
            return_error(invocation, error);
            return;
        }
        g_autofree gchar *config_path = write_runtime_config(
            id, url, title, user_agent, width, height, maximized,
            start_in_background, policy, &error);
        if (config_path == NULL ||
            !spawn_runtime(id, config_path, lock_fd, &error)) {
            close(lock_fd);
            if (config_path != NULL)
                unlink(config_path);
            return_error(invocation, error);
            return;
        }
        g_dbus_method_invocation_return_value(invocation, NULL);
        return;
    }
    if (g_str_equal(method_name, "DeleteProfile")) {
        const gchar *id, *token;
        g_variant_get(parameters, "(&s&s)", &id, &token);
        if (!valid_app_id(id) || !valid_token(token)) {
            g_set_error_literal(&error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                                "invalid DeleteProfile request");
            return_error(invocation, error);
            return;
        }
        if (!delete_profile(id, token, &error)) {
            return_error(invocation, error);
            return;
        }
        g_dbus_method_invocation_return_value(invocation, NULL);
        return;
    }
    g_dbus_method_invocation_return_error(
        invocation, G_DBUS_ERROR, G_DBUS_ERROR_UNKNOWN_METHOD,
        "unknown Chromium engine method %s", method_name);
}

static const GDBusInterfaceVTable interface_vtable = {
    .method_call = handle_method_call,
};

static void
on_bus_acquired(GDBusConnection *connection, const gchar *name, gpointer user_data)
{
    (void) name;
    (void) user_data;
    g_autoptr(GError) error = NULL;
    g_autoptr(GDBusNodeInfo) info =
        g_dbus_node_info_new_for_xml(introspection_xml, &error);
    if (info == NULL ||
        g_dbus_connection_register_object(connection, OBJECT_PATH,
                                          info->interfaces[0],
                                          &interface_vtable, NULL, NULL,
                                          &error) == 0) {
        g_printerr("Failed to register Chromium engine object: %s\n",
                   error != NULL ? error->message : "unknown error");
        g_main_loop_quit(main_loop);
    }
}

static void
on_name_lost(GDBusConnection *connection, const gchar *name, gpointer user_data)
{
    (void) connection;
    (void) name;
    (void) user_data;
    if (main_loop != NULL)
        g_main_loop_quit(main_loop);
}

static int
run_runtime(const gchar *id, const gchar *config_path)
{
    if (!valid_app_id(id)) {
        g_printerr("Invalid Bastle app ID\n");
        return 2;
    }
    g_autofree gchar *runtime_dir =
        g_build_filename(g_get_user_runtime_dir(), "bastle-chromium", NULL);
    g_autofree gchar *canonical_dir = g_canonicalize_filename(runtime_dir, NULL);
    g_autofree gchar *canonical_config = g_canonicalize_filename(config_path, NULL);
    g_autofree gchar *prefix = g_strconcat(canonical_dir, G_DIR_SEPARATOR_S, NULL);
    if (!g_str_has_prefix(canonical_config, prefix)) {
        g_printerr("Runtime request is outside the private runtime directory\n");
        return 2;
    }
    int lock_fd = -1;
    g_autoptr(GError) lock_error = NULL;
    if (!open_profile_lock(id, LOCK_SH, &lock_fd, &lock_error)) {
        g_printerr("Chromium profile is being deleted\n");
        return 1;
    }
    g_autofree gchar *profile = profile_path(id);
    if (g_mkdir_with_parents(profile, 0700) != 0) {
        g_printerr("Cannot create Chromium profile: %s\n", g_strerror(errno));
        close(lock_fd);
        return 1;
    }
    g_setenv("BASTLE_CHROMIUM_ID", id, TRUE);
    g_autofree gchar *argument =
        g_strdup_printf("--bastle-config=%s", canonical_config);
    const gchar *wayland_display = g_getenv("WAYLAND_DISPLAY");
    const gchar *ozone_platform =
        wayland_display != NULL && *wayland_display != '\0'
            ? "--ozone-platform=wayland"
            : "--ozone-platform=x11";
    execlp("zypak-wrapper", "zypak-wrapper",
           "/app/lib/bastle-chromium/electron",
           ozone_platform, "/app/lib/bastle-chromium/main.js", argument, NULL);
    g_printerr("Cannot start Electron: %s\n", g_strerror(errno));
    close(lock_fd);
    return 1;
}

static int
self_test(void)
{
    g_autoptr(GError) error = NULL;
    g_autoptr(JsonNode) policy = NULL;
    g_autoptr(GString) max_title = g_string_sized_new(512 * 4);
    for (guint index = 0; index < 512; index++)
        g_string_append_unichar(max_title, 0x1f3e0);
    gboolean max_title_valid = valid_title(max_title->str);
    g_string_append_unichar(max_title, 0x1f3e0);
    gboolean oversized_title_valid = valid_title(max_title->str);
    if (!valid_app_id("abcdefghijkl") || valid_app_id("../bad-value") ||
        !valid_token("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef") ||
        valid_token("../bad") || !valid_http_url("https://example.org/path") ||
        valid_http_url("file:///etc/passwd") || !valid_title("Example") ||
        valid_title("Bad\nTitle") || !max_title_valid || oversized_title_valid ||
        !valid_policy("{\"schema_version\":2}", &policy, &error)) {
        g_printerr("Chromium engine validation self-test failed\n");
        return 1;
    }

    g_autofree gchar *test_root =
        g_dir_make_tmp("bastle-chromium-test-XXXXXX", &error);
    if (test_root == NULL || !g_setenv("XDG_DATA_HOME", test_root, TRUE) ||
        !g_setenv("XDG_CACHE_HOME", test_root, TRUE)) {
        g_printerr("Cannot create Chromium engine lock test directory\n");
        return 1;
    }
    const gchar *token =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    int shared_lock = -1;
    int exclusive_lock = -1;
    if (!open_profile_lock("abcdefghijkl", LOCK_SH, &shared_lock, &error) ||
        !ensure_token("abcdefghijkl", token, TRUE, &error)) {
        g_printerr("Cannot prepare Chromium engine lock test: %s\n", error->message);
        return 1;
    }
    g_clear_error(&error);
    if (delete_profile("abcdefghijkl", token, &error) ||
        !g_error_matches(error, G_IO_ERROR, G_IO_ERROR_BUSY)) {
        g_printerr("Profile deletion was not serialized with app opening\n");
        close(shared_lock);
        return 1;
    }
    g_clear_error(&error);
    close(shared_lock);
    if (!delete_profile("abcdefghijkl", token, &error) ||
        !open_profile_lock("abcdefghijkl", LOCK_EX, &exclusive_lock, &error)) {
        g_printerr("Serialized profile deletion failed: %s\n", error->message);
        return 1;
    }
    close(exclusive_lock);
    if (!remove_tree(test_root, &error)) {
        g_printerr("Cannot clean Chromium engine lock test: %s\n", error->message);
        return 1;
    }
    return 0;
}

int
main(int argc, char **argv)
{
    if (argc == 2 && g_str_equal(argv[1], "--self-test"))
        return self_test();
    if (argc == 4 && g_str_equal(argv[1], "--runtime"))
        return run_runtime(argv[2], argv[3]);
    if (argc != 1) {
        g_printerr("Usage: %s [--self-test | --runtime ID CONFIG]\n", argv[0]);
        return 2;
    }

    main_loop = g_main_loop_new(NULL, FALSE);
    guint owner_id = g_bus_own_name(
        G_BUS_TYPE_SESSION, BUS_NAME,
        G_BUS_NAME_OWNER_FLAGS_NONE,
        on_bus_acquired, NULL, on_name_lost, NULL, NULL);
    g_main_loop_run(main_loop);
    g_bus_unown_name(owner_id);
    g_main_loop_unref(main_loop);
    return 0;
}
