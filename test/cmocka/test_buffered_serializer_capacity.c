#include <stdarg.h>
#include <stddef.h>
#include <setjmp.h>
#include <cmocka.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <util/bmem.h>
#include <util/buffered-file-serializer.h>
#include <util/platform.h>
#include <util/serializer.h>

#define MIB ((size_t)1048576)

struct paths {
	char directory[96];
	char file[128];
};

static int setup(void **state)
{
	struct paths *paths = calloc(1, sizeof(*paths));
	assert_non_null(paths);
	char *id = os_generate_uuid();
	assert_non_null(id);
	snprintf(paths->directory, sizeof(paths->directory), "test_buffered_serializer_capacity-%s", id);
	bfree(id);
	assert_int_equal(os_mkdir(paths->directory), MKDIR_SUCCESS);
	snprintf(paths->file, sizeof(paths->file), "%s/output.bin", paths->directory);
	*state = paths;
	return 0;
}

static int teardown(void **state)
{
	struct paths *paths = *state;
	if (os_file_exists(paths->file))
		assert_int_equal(os_unlink(paths->file), 0);
	assert_int_equal(os_rmdir(paths->directory), 0);
	free(paths);
	return 0;
}

static void roundtrip(void **state, size_t buffer_size, size_t chunk_size, size_t length)
{
	struct paths *paths = *state;
	unsigned char *expected = malloc(length);
	unsigned char *actual = malloc(length);
	assert_non_null(expected);
	assert_non_null(actual);
	for (size_t i = 0; i < length; i++)
		expected[i] = (unsigned char)(i ^ (i >> 8));

	struct serializer serializer = {0};
	assert_true(buffered_file_serializer_init(&serializer, paths->file, buffer_size, chunk_size));
	assert_int_equal(serializer_get_pos(&serializer), 0);
	assert_int_equal(s_write(&serializer, expected, length), length);
	assert_int_equal(serializer_get_pos(&serializer), length);

	size_t middle = length / 2;
	assert_int_equal(serializer_seek(&serializer, (int64_t)middle, SERIALIZE_SEEK_START), middle);
	assert_int_equal(s_write(&serializer, "new", 3), 3);
	assert_int_equal(serializer_get_pos(&serializer), middle + 3);
	memcpy(expected + middle, "new", 3);
	buffered_file_serializer_free(&serializer);

	FILE *file = os_fopen(paths->file, "rb");
	assert_non_null(file);
	assert_int_equal(fread(actual, 1, length, file), length);
	assert_int_equal(fgetc(file), EOF);
	assert_int_equal(fclose(file), 0);
	assert_memory_equal(actual, expected, length);
	free(actual);
	free(expected);
}

static void chunk_equal_to_buffer(void **state)
{
	roundtrip(state, MIB, MIB, 3 * MIB + 37);
}

static void default_chunk_with_small_buffer(void **state)
{
	roundtrip(state, MIB, 0, MIB + 37);
}

static void chunk_larger_than_buffer(void **state)
{
	roundtrip(state, MIB, 4 * MIB, 4 * MIB + 37);
}

static void small_chunks_keep_working(void **state)
{
	roundtrip(state, 64, 8, 1000);
}

static void defaults_keep_working(void **state)
{
	roundtrip(state, 0, 0, 2 * MIB + 37);
}

int main(int argc, char **argv)
{
	const struct CMUnitTest tests[] = {
		cmocka_unit_test_setup_teardown(chunk_equal_to_buffer, setup, teardown),
		cmocka_unit_test_setup_teardown(default_chunk_with_small_buffer, setup, teardown),
		cmocka_unit_test_setup_teardown(chunk_larger_than_buffer, setup, teardown),
		cmocka_unit_test_setup_teardown(small_chunks_keep_working, setup, teardown),
		cmocka_unit_test_setup_teardown(defaults_keep_working, setup, teardown),
	};
	if (argc == 2) {
		bool found = false;
		for (size_t i = 0; i < sizeof(tests) / sizeof(tests[0]); i++)
			found |= strcmp(argv[1], tests[i].name) == 0;
		if (!found) {
			fprintf(stderr, "unknown test: %s\n", argv[1]);
			return 2;
		}
		cmocka_set_test_filter(argv[1]);
	} else if (argc != 1) {
		return 2;
	}
	return cmocka_run_group_tests(tests, NULL, NULL);
}

#undef MIB
