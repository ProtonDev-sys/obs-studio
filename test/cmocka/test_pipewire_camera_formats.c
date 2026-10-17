/* SPDX-License-Identifier: GPL-2.0-or-later */
#include <stdarg.h>
#include <stddef.h>
#include <setjmp.h>
#include <cmocka.h>

/* Exercise the production enumeration and framerate callbacks with real SPA
 * PODs, without needing a camera, portal, PipeWire daemon or graphics context. */
#include "../../plugins/linux-pipewire/camera-portal.c.h"

const char *obs_module_text(const char *text)
{
	return text;
}

struct format_fixture {
	struct camera_device device;
	obs_properties_t *properties;
	obs_property_t *formats;
	obs_property_t *framerates;
};

static int setup(void **state)
{
	struct format_fixture *fixture = bzalloc(sizeof(*fixture));
	spa_list_init(&fixture->device.param_list);
	fixture->properties = obs_properties_create();
	fixture->formats = obs_properties_add_list(fixture->properties, "format", "Format", OBS_COMBO_TYPE_LIST,
						   OBS_COMBO_FORMAT_STRING);
	fixture->framerates = obs_properties_add_list(fixture->properties, "framerate", "Framerate",
						      OBS_COMBO_TYPE_LIST, OBS_COMBO_FORMAT_STRING);
	*state = fixture;
	return 0;
}

static int teardown(void **state)
{
	struct format_fixture *fixture = *state;
	clear_params(&fixture->device.param_list, SPA_ID_INVALID);
	obs_properties_destroy(fixture->properties);
	bfree(fixture);
	return 0;
}

static void add_format(struct format_fixture *fixture, uint32_t format, uint32_t width, uint32_t height,
		       const struct spa_fraction *framerates, uint32_t n_framerates)
{
	uint64_t buffer[128];
	struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(buffer, sizeof(buffer));
	struct spa_pod_frame object;

	spa_pod_builder_push_object(&builder, &object, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat);
	spa_pod_builder_add(&builder, SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video), SPA_FORMAT_mediaSubtype,
			    SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw), SPA_FORMAT_VIDEO_format, SPA_POD_Id(format), 0);
	spa_pod_builder_prop(&builder, SPA_FORMAT_VIDEO_size, 0);
	spa_pod_builder_rectangle(&builder, width, height);
	if (n_framerates != 0) {
		spa_pod_builder_prop(&builder, SPA_FORMAT_VIDEO_framerate, 0);
		if (n_framerates == 1) {
			spa_pod_builder_fraction(&builder, framerates[0].num, framerates[0].denom);
		} else {
			struct spa_pod_frame choice;
			spa_pod_builder_push_choice(&builder, &choice, SPA_CHOICE_Enum, 0);
			/* The default is followed by every selectable value, including itself. */
			spa_pod_builder_fraction(&builder, framerates[0].num, framerates[0].denom);
			for (uint32_t i = 0; i < n_framerates; i++)
				spa_pod_builder_fraction(&builder, framerates[i].num, framerates[i].denom);
			spa_pod_builder_pop(&builder, &choice);
		}
	}
	struct spa_pod *pod = spa_pod_builder_pop(&builder, &object);
	assert_non_null(pod);
	assert_non_null(add_param(&fixture->device.param_list, 0, SPA_PARAM_EnumFormat, pod));
}

static void assert_format(obs_property_t *property, size_t index, const char *name, uint32_t format, uint32_t width,
			  uint32_t height)
{
	assert_string_equal(obs_property_list_item_name(property, index), name);
	obs_data_t *data = obs_data_create_from_json(obs_property_list_item_string(property, index));
	assert_non_null(data);
	assert_false(obs_data_get_bool(data, "encoded"));
	assert_int_equal(obs_data_get_int(data, "video_format"), format);
	assert_int_equal(obs_data_get_int(data, "width"), width);
	assert_int_equal(obs_data_get_int(data, "height"), height);
	obs_data_release(data);
}

static void libcamera_default_is_listed_once(void **state)
{
	struct format_fixture *fixture = *state;
	/* PipeWire 1.0.5's spa_libcamera_enum_format() emits the preferred size
	 * first, then all supported sizes (including that size again). It does
	 * not advertise a framerate. This is the duplicate in #1068. */
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 640, 480, NULL, 0);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 320, 240, NULL, 0);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 640, 480, NULL, 0);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 1280, 720, NULL, 0);

	for (unsigned refresh = 0; refresh < 2; refresh++) {
		camera_format_list(&fixture->device, fixture->formats);
		assert_int_equal(obs_property_list_item_count(fixture->formats), 3);
		assert_format(fixture->formats, 0, "640x480 (4:3) - NV12", SPA_VIDEO_FORMAT_NV12, 640, 480);
		assert_format(fixture->formats, 1, "320x240 (4:3) - NV12", SPA_VIDEO_FORMAT_NV12, 320, 240);
		assert_format(fixture->formats, 2, "1280x720 (16:9) - NV12", SPA_VIDEO_FORMAT_NV12, 1280, 720);
	}
}

static void missing_framerate_does_not_invent_30_fps(void **state)
{
	struct format_fixture *fixture = *state;
	struct spa_rectangle resolution = SPA_RECTANGLE(640, 480);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 640, 480, NULL, 0);
	obs_property_list_add_string(fixture->framerates, "stale", "stale");

	camera_format_list(&fixture->device, fixture->formats);
	assert_int_equal(obs_property_list_item_count(fixture->formats), 1);
	assert_format(fixture->formats, 0, "640x480 (4:3) - NV12", SPA_VIDEO_FORMAT_NV12, 640, 480);
	assert_false(framerate_list(&fixture->device, SPA_VIDEO_FORMAT_NV12, &resolution, fixture->framerates));
	assert_int_equal(obs_property_list_item_count(fixture->framerates), 0);
}

static void nv12_framerates_use_the_spa_format_id(void **state)
{
	struct format_fixture *fixture = *state;
	const struct spa_fraction fps30 = SPA_FRACTION(30, 1);
	const struct spa_fraction fps60 = SPA_FRACTION(60, 1);
	struct spa_rectangle resolution = SPA_RECTANGLE(640, 480);
	struct spa_fraction selected;
	/* The empty-list part of #1068 was already fixed: OBS and SPA pixel
	 * format enum values must not be compared to each other. */
	assert_int_not_equal(SPA_VIDEO_FORMAT_NV12, VIDEO_FORMAT_NV12);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 640, 480, &fps30, 1);
	add_format(fixture, SPA_VIDEO_FORMAT_YUY2, 640, 480, &fps60, 1);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 1280, 720, &fps60, 1);

	camera_format_list(&fixture->device, fixture->formats);
	assert_int_equal(obs_property_list_item_count(fixture->formats), 3);
	assert_format(fixture->formats, 0, "640x480 (4:3) - 30 FPS - NV12", SPA_VIDEO_FORMAT_NV12, 640, 480);
	assert_true(framerate_list(&fixture->device, SPA_VIDEO_FORMAT_NV12, &resolution, fixture->framerates));
	assert_int_equal(obs_property_list_item_count(fixture->framerates), 1);
	assert_string_equal(obs_property_list_item_name(fixture->framerates, 0), "30.00");
	assert_true(parse_framerate(&selected, obs_property_list_item_string(fixture->framerates, 0)));
	assert_int_equal(selected.num, 30);
	assert_int_equal(selected.denom, 1);
}

static void distinct_framerate_descriptions_are_preserved(void **state)
{
	struct format_fixture *fixture = *state;
	const struct spa_fraction fps30 = SPA_FRACTION(30, 1);
	const struct spa_fraction rates[] = {SPA_FRACTION(30, 1), SPA_FRACTION(60, 1)};
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 640, 480, &fps30, 1);
	add_format(fixture, SPA_VIDEO_FORMAT_NV12, 640, 480, rates, 2);

	camera_format_list(&fixture->device, fixture->formats);
	assert_int_equal(obs_property_list_item_count(fixture->formats), 2);
	assert_format(fixture->formats, 0, "640x480 (4:3) - 30 FPS - NV12", SPA_VIDEO_FORMAT_NV12, 640, 480);
	assert_format(fixture->formats, 1, "640x480 (4:3) - 60, 30 FPS - NV12", SPA_VIDEO_FORMAT_NV12, 640, 480);
}

int main(void)
{
	const struct CMUnitTest tests[] = {
		cmocka_unit_test_setup_teardown(libcamera_default_is_listed_once, setup, teardown),
		cmocka_unit_test_setup_teardown(missing_framerate_does_not_invent_30_fps, setup, teardown),
		cmocka_unit_test_setup_teardown(nv12_framerates_use_the_spa_format_id, setup, teardown),
		cmocka_unit_test_setup_teardown(distinct_framerate_descriptions_are_preserved, setup, teardown),
	};
	return cmocka_run_group_tests(tests, NULL, NULL);
}
