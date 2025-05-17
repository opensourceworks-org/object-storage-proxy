FROM apache/hive:4.0.0

USER root
RUN apt update && apt install -y sudo wget curl
COPY ./jars /opt/hive/aws-jars

# add hive user to sudoers
RUN echo "hive ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers
RUN chown -R hive:hive /opt/hive
RUN cp /opt/hive/aws-jars/* /opt/hive/lib/ && cp /opt/hive/aws-jars/* /opt/hadoop/share/hadoop/common/lib/ &&\
    cp /opt/hive/aws-jars/* /opt/hadoop/share/hadoop/common/ && \
    cp /opt/hive/aws-jars/* /opt/hadoop/share/hadoop/hdfs/lib/ && \
    cp /opt/hive/aws-jars/* /opt/hadoop/share/hadoop/hdfs/ && \
    cp /opt/hive/aws-jars/* /opt/hadoop/share/hadoop/yarn/lib/ && \
    cp /opt/hive/aws-jars/* /opt/hadoop/share/hadoop/yarn/

USER hive