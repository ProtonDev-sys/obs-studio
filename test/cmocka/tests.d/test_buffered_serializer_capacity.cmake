add_executable(test_buffered_serializer_capacity test_buffered_serializer_capacity.c)
target_include_directories(test_buffered_serializer_capacity PRIVATE ${CMOCKA_INCLUDE_DIR})
target_link_libraries(test_buffered_serializer_capacity PRIVATE OBS::libobs ${CMOCKA_LIBRARIES})
add_dependencies(cmocka-tests test_buffered_serializer_capacity)

# Run cases separately: a blocked write is a timeout, while allocation,
# initialization, and content errors retain their ordinary failure status.
foreach(
  _buffered_case
  IN
  ITEMS
    chunk_equal_to_buffer
    default_chunk_with_small_buffer
    chunk_larger_than_buffer
    small_chunks_keep_working
    defaults_keep_working
)
  add_test(
    NAME test_buffered_serializer_capacity_${_buffered_case}
    COMMAND $<TARGET_FILE:test_buffered_serializer_capacity> ${_buffered_case}
  )
  set_tests_properties(test_buffered_serializer_capacity_${_buffered_case} PROPERTIES TIMEOUT 15)
endforeach()
