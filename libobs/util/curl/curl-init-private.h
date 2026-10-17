/* Private to OBS's in-tree HTTP clients; not part of the installed libobs API. */

#pragma once

#include <curl/curl.h>

static inline CURL *curl_obs_easy_init(void)
{
	return curl_easy_init();
}
