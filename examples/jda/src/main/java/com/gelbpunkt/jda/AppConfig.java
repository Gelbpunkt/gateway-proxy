package com.gelbpunkt.jda;

import lombok.Data;
import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.stereotype.Component;

@Data
@Component
@ConfigurationProperties(prefix = "app")
public class AppConfig {

    private String token;
    private Integer shards;
    private String gateway;

}
