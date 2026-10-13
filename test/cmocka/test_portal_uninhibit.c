/* SPDX-License-Identifier: GPL-2.0-or-later */

#include <gio/gio.h>
#include <util/bmem.h>

static GTask *pending;
static unsigned completed;
static unsigned warnings;

static void queue_call(GDBusConnection *connection, const gchar *bus_name, const gchar *object_path,
		       const gchar *interface_name, const gchar *method_name, GVariant *parameters,
		       const GVariantType *reply_type, GDBusCallFlags flags, gint timeout_msec, GCancellable *cancellable,
		       GAsyncReadyCallback callback, gpointer user_data);
static GVariant *finish_call(GDBusConnection *connection, GAsyncResult *result, GError **error);

/* Keep real GTask dispatch and source-object ownership, but control when the
 * D-Bus reply arrives. Compile the production callback into this ASan target. */
#define g_dbus_connection_call queue_call
#define g_dbus_connection_call_finish finish_call
#include "../../libobs/util/platform-nix-portal.c"
#undef g_dbus_connection_call
#undef g_dbus_connection_call_finish

static void queue_call(GDBusConnection *connection, const gchar *bus_name, const gchar *object_path,
		       const gchar *interface_name, const gchar *method_name, GVariant *parameters,
		       const GVariantType *reply_type, GDBusCallFlags flags, gint timeout_msec, GCancellable *cancellable,
		       GAsyncReadyCallback callback, gpointer user_data)
{
	UNUSED_PARAMETER(bus_name);
	UNUSED_PARAMETER(object_path);
	UNUSED_PARAMETER(interface_name);
	UNUSED_PARAMETER(reply_type);
	UNUSED_PARAMETER(flags);
	UNUSED_PARAMETER(timeout_msec);
	g_assert_cmpstr(method_name, ==, "Close");
	g_assert_null(pending);
	pending = g_task_new(connection, cancellable, callback, user_data);
	g_variant_unref(g_variant_ref_sink(parameters));
}

static GVariant *finish_call(GDBusConnection *connection, GAsyncResult *result, GError **error)
{
	g_assert_true(connection == g_task_get_source_object(G_TASK(result)));
	completed++;
	return g_task_propagate_pointer(G_TASK(result), error);
}

void blog(int log_level, const char *format, ...)
{
	UNUSED_PARAMETER(format);
	if (log_level == LOG_WARNING)
		warnings++;
}

static void reply_after_destroy(bool fail)
{
	unsigned before = completed;
	struct portal_inhibit_info *info = bzalloc(sizeof(*info));
	/* The transport is mocked; a real, unconnected GDBusConnection suffices. */
	info->c = g_object_new(G_TYPE_DBUS_CONNECTION, NULL);
	info->request_path = bstrdup("/org/freedesktop/portal/desktop/request/test/inhibit");
	info->active = true;

	do_uninhibit(info);
	portal_inhibit_info_destroy(info);
	g_assert_nonnull(pending);
	g_assert_cmpuint(completed, ==, before);

	if (fail)
		g_task_return_new_error(pending, G_IO_ERROR, G_IO_ERROR_FAILED, "test reply failure");
	else
		g_task_return_pointer(pending, g_variant_ref_sink(g_variant_new("()")), (GDestroyNotify)g_variant_unref);
	g_clear_object(&pending);
	while (completed == before)
		g_main_context_iteration(NULL, TRUE);
}

int main(void)
{
	reply_after_destroy(false);
	reply_after_destroy(true);
	g_assert_cmpuint(completed, ==, 2);
	g_assert_cmpuint(warnings, ==, 1);
	return 0;
}
