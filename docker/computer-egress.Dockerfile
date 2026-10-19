FROM python:3.11-slim
COPY docker/computer-egress.py /opt/qunica/egress.py
USER 10001:10001
ENTRYPOINT ["python3", "-B", "/opt/qunica/egress.py"]
