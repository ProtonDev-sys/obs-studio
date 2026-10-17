/* Private to OBS's in-tree HTTP clients; not part of the installed libobs API. */

#pragma once

#include <curl/curl.h>
#include <stdlib.h>

static inline CURL *curl_obs_easy_init(void)
{
	CURL *curl = curl_easy_init();
	if (!curl)
		return NULL;

	/* AppRun selects the host's certificate bundle. libcurl does not read
	 * SSL_CERT_FILE itself, and its compiled-in path belongs to the build
	 * distribution. Other launchers retain libcurl's native defaults. */
	const char *ca_bundle = getenv("OBS_CURL_CA_BUNDLE");
	if (ca_bundle && *ca_bundle && curl_easy_setopt(curl, CURLOPT_CAINFO, ca_bundle) != CURLE_OK) {
		curl_easy_cleanup(curl);
		return NULL;
	}
	return curl;
}
