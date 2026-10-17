/* Local TLS fixture for test-apprun-curl.sh. Built alongside the AppImage,
 * then run next to its GUI with the same relative RPATH and private curl
 * initializer as OBS. No external endpoint or system-installed libcurl is
 * used. The server binds an ephemeral loopback port and prints its URL only
 * after listen() succeeds. */

#include <arpa/inet.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

#include <openssl/err.h>
#include <openssl/ssl.h>

#include <util/curl/curl-init-private.h>

static const char response_body[] = "OBS verified local TLS\n";

static int serve(const char *certificate, const char *key)
{
	SSL_CTX *context = SSL_CTX_new(TLS_server_method());
	if (!context || SSL_CTX_use_certificate_chain_file(context, certificate) != 1 ||
	    SSL_CTX_use_PrivateKey_file(context, key, SSL_FILETYPE_PEM) != 1 ||
	    SSL_CTX_check_private_key(context) != 1) {
		ERR_print_errors_fp(stderr);
		SSL_CTX_free(context);
		return 1;
	}

	int listener = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
	struct sockaddr_in address = {.sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
	socklen_t address_size = sizeof(address);
	if (listener < 0 || bind(listener, (struct sockaddr *)&address, sizeof(address)) < 0 ||
	    listen(listener, 8) < 0 || getsockname(listener, (struct sockaddr *)&address, &address_size) < 0) {
		perror("TLS fixture listener");
		if (listener >= 0)
			close(listener);
		SSL_CTX_free(context);
		return 1;
	}

	printf("https://localhost:%u/\n", ntohs(address.sin_port));
	fflush(stdout);
	for (;;) {
		int client = accept(listener, NULL, NULL);
		if (client < 0) {
			if (errno == EINTR)
				continue;
			perror("TLS fixture accept");
			break;
		}
		struct timeval timeout = {.tv_sec = 5};
		if (setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) < 0 ||
		    setsockopt(client, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)) < 0) {
			perror("TLS fixture timeout");
			close(client);
			break;
		}
		SSL *connection = SSL_new(context);
		if (!connection) {
			ERR_print_errors_fp(stderr);
			close(client);
			break;
		}
		if (SSL_set_fd(connection, client) == 1 && SSL_accept(connection) == 1) {
			char request[4096];
			if (SSL_read(connection, request, sizeof(request)) > 0) {
				char response[256];
				int size = snprintf(response, sizeof(response),
						    "HTTP/1.1 200 OK\r\nContent-Length: %zu\r\n"
						    "Connection: close\r\n\r\n%s",
						    sizeof(response_body) - 1, response_body);
				SSL_write(connection, response, size);
			}
		}
		/* Rejected certificates/hostnames deliberately abort the handshake.
		 * They must not stop the fixture or require a readiness sleep. */
		SSL_free(connection);
		close(client);
	}
	close(listener);
	SSL_CTX_free(context);
	return 1;
}

struct response {
	char body[sizeof(response_body)];
	size_t size;
};

static size_t receive_response(char *data, size_t size, size_t count, void *opaque)
{
	struct response *response = opaque;
	size_t bytes = size * count;
	if (bytes > sizeof(response->body) - 1 - response->size)
		return 0;
	memcpy(response->body + response->size, data, bytes);
	response->size += bytes;
	return bytes;
}

static int get(const char *url, int raw)
{
	if (curl_global_init(CURL_GLOBAL_ALL) != CURLE_OK)
		return CURLE_FAILED_INIT;
	CURL *curl = raw ? curl_easy_init() : curl_obs_easy_init();
	if (!curl) {
		curl_global_cleanup();
		return CURLE_FAILED_INIT;
	}
	char error[CURL_ERROR_SIZE] = {0};
	struct response response = {0};
	curl_easy_setopt(curl, CURLOPT_URL, url);
	curl_easy_setopt(curl, CURLOPT_NOPROXY, "*");
	curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 3L);
	curl_easy_setopt(curl, CURLOPT_TIMEOUT, 5L);
	curl_easy_setopt(curl, CURLOPT_ERRORBUFFER, error);
	curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, receive_response);
	curl_easy_setopt(curl, CURLOPT_WRITEDATA, &response);
	/* Keep libcurl's certificate and hostname verification defaults. */
	CURLcode result = curl_easy_perform(curl);
	const char *ca = getenv("OBS_CURL_CA_BUNDLE");
	fprintf(stderr, "%s\nOBS_CURL_CA_BUNDLE=%s\nresult=%d: %s\n", curl_version(), ca ? ca : "(unset)", result,
		error[0] ? error : curl_easy_strerror(result));
	curl_easy_cleanup(curl);
	curl_global_cleanup();
	if (result != CURLE_OK)
		return result;
	if (response.size != sizeof(response_body) - 1 || strcmp(response.body, response_body) != 0) {
		fprintf(stderr, "unexpected TLS fixture response\n");
		return CURLE_WEIRD_SERVER_REPLY;
	}
	fputs(response.body, stdout);
	return 0;
}

int main(int argc, char **argv)
{
	signal(SIGPIPE, SIG_IGN);
	if (argc == 4 && strcmp(argv[1], "serve") == 0)
		return serve(argv[2], argv[3]);
	if (argc == 3 && strcmp(argv[1], "get") == 0)
		return get(argv[2], 0);
	if (argc == 3 && strcmp(argv[1], "raw") == 0)
		return get(argv[2], 1);
	fprintf(stderr, "usage: %s serve <certificate> <key> | get <url> | raw <url>\n", argv[0]);
	return 2;
}
