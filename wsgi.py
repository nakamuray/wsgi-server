'''example WSGI application module
'''


def application(environ, start_response):
    body = b'Hello World!\n'
    status = '200 OK'
    headers = [
        ('Content-Type', 'text/plain'),
        ('Content-Length', str(len(body))),
    ]

    start_response(status, headers)

    return [body]
