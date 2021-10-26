package com.gelbpunkt.jda;

import lombok.extern.slf4j.Slf4j;
import net.dv8tion.jda.api.requests.GatewayIntent;
import net.dv8tion.jda.api.sharding.DefaultShardManagerBuilder;
import net.dv8tion.jda.api.sharding.ShardManager;
import net.dv8tion.jda.api.utils.Compression;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.context.annotation.Bean;

import javax.security.auth.login.LoginException;

@Slf4j
@SpringBootApplication
public class Application {

    public static void main(String[] args) {
        SpringApplication.run(Application.class, args);
    }

    @Bean
    public ShardManager jda(AppConfig config) {
        try {
            ShardManager manager = DefaultShardManagerBuilder
                    .createDefault(config.getToken())
                    .setSessionController(new GatewayController(config))
                    .enableIntents(GatewayIntent.GUILD_MEMBERS)
                    .setCompression(Compression.NONE)
                    .setShardsTotal(config.getShards())
                    .build();
            log.info("Intents: " + GatewayIntent.getRaw(manager.getGatewayIntents()));
            return manager;
        } catch (LoginException ex) {
            throw new IllegalStateException("Failed to start JDA", ex);
        }
    }

}
