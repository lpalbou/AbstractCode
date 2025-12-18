# RType-style Arcade Game in Python using Pygame
# Run with: python3 main.py

import pygame
import sys
import random
import math

# Initialize Pygame
pygame.init()

# Constants (RType is horizontal shooter)
WIDTH, HEIGHT = 1024, 768
FPS = 60
WHITE = (255, 255, 255)
BLACK = (0, 0, 0)
RED = (255, 0, 0)
GREEN = (0, 255, 0)
BLUE = (0, 0, 255)
YELLOW = (255, 255, 0)

# Setup display
screen = pygame.display.set_mode((WIDTH, HEIGHT))
pygame.display.set_caption("RType - Horizontal Shooter")
clock = pygame.time.Clock()

# Fonts
font_small = pygame.font.Font(None, 24)
font_large = pygame.font.Font(None, 72)

# Player class - moves vertically, shoots horizontally
class Player(pygame.sprite.Sprite):
    def __init__(self):
        super().__init__()
        self.image = pygame.Surface((40, 30))
        self.image.fill(BLUE)
        pygame.draw.polygon(self.image, BLUE, [(0, 15), (40, 10), (40, 20)])  # Ship pointing right
        self.rect = self.image.get_rect()
        self.rect.left = 100  # Fixed horizontal position (player doesn't move left/right)
        self.rect.centery = HEIGHT // 2
        self.speed_y = 5
        self.health = 100
        self.score = 0
        self.power_level = 0  # 0: normal, 1: force pod, 2: laser
        self.force_pod = None
        self.last_shot = 0
        self.shoot_delay = 300  # milliseconds

    def update(self):
        keys = pygame.key.get_pressed()
        if keys[pygame.K_UP] and self.rect.top > 0:
            self.rect.y -= self.speed_y
        if keys[pygame.K_DOWN] and self.rect.bottom < HEIGHT:
            self.rect.y += self.speed_y

        # Shoot (horizontal, to the right)
        current_time = pygame.time.get_ticks()
        if keys[pygame.K_SPACE] and current_time - self.last_shot > self.shoot_delay:
            self.shoot()
            self.last_shot = current_time

        # Update force pod position if attached
        if self.force_pod:
            self.force_pod.update_position(self.rect.centerx, self.rect.centery)

    def shoot(self):
        if self.power_level == 0:
            bullet = Bullet(self.rect.right, self.rect.centery, "normal")
            all_sprites.add(bullet)
            bullets.add(bullet)
        elif self.power_level == 1:
            # Force pod: center + side shots
            bullet = Bullet(self.rect.right, self.rect.centery - 5, "normal")
            all_sprites.add(bullet)
            bullets.add(bullet)
            side_bullet1 = Bullet(self.rect.right, self.rect.top + 5, "side")
            all_sprites.add(side_bullet1)
            bullets.add(side_bullet1)
            side_bullet2 = Bullet(self.rect.right, self.rect.bottom - 5, "side")
            all_sprites.add(side_bullet2)
            bullets.add(side_bullet2)
        elif self.power_level == 2:
            # Laser: wide horizontal beam
            for i in range(-3, 4):
                bullet = Bullet(self.rect.right + i * 5, self.rect.centery, "laser")
                all_sprites.add(bullet)
                bullets.add(bullet)

    def add_powerup(self, power_type):
        if power_type == "force_pod":
            self.power_level = 1
            self.force_pod = ForcePod(self.rect.centerx, self.rect.centery)
            all_sprites.add(self.force_pod)
        elif power_type == "laser":
            self.power_level = 2
        elif power_type == "speed":
            self.speed_y *= 1.5

    def take_damage(self, damage):
        self.health -= damage
        if self.health <= 0:
            self.kill()

class Bullet(pygame.sprite.Sprite):
    def __init__(self, x, y, type_="normal"):
        super().__init__()
        self.type = type_
        if type_ == "normal":
            self.image = pygame.Surface((15, 5))
            self.image.fill(WHITE)
            self.speed = 8
        elif type_ == "side":
            self.image = pygame.Surface((10, 5))
            self.image.fill(YELLOW)
            self.speed = 6
            self.vel_y = random.choice([-2, 2])
        elif type_ == "laser":
            self.image = pygame.Surface((25, 8))
            self.image.fill(GREEN)
            self.speed = 10
            self.vel_y = 0
        
        self.rect = self.image.get_rect()
        self.rect.centery = y
        self.rect.left = x  # Starts from player's right edge
        
    def update(self):
        self.rect.x += self.speed  # Move right (horizontal scroll direction)
        if self.type == "side":
            self.rect.y += self.vel_y
        
        # Remove if off screen (right edge)
        if self.rect.left > WIDTH:
            self.kill()

class Enemy(pygame.sprite.Sprite):
    def __init__(self, x, y, enemy_type="basic"):
        super().__init__()
        self.enemy_type = enemy_type
        if enemy_type == "basic":
            self.image = pygame.Surface((30, 20))
            self.image.fill(RED)
            pygame.draw.polygon(self.image, RED, [(0, 10), (30, 5), (30, 15)])
            self.health = 1
            self.speed_x = -2  # Move left (toward player)
            self.score_value = 10
        elif enemy_type == "boss":
            self.image = pygame.Surface((80, 60))
            self.image.fill(RED)
            pygame.draw.rect(self.image, RED, (10, 5, 60, 40))
            pygame.draw.polygon(self.image, RED, [(15, 20), (25, 10), (75, 10), (85, 20)])
            self.health = 10
            self.speed_x = -1.5
            self.score_value = 100
        
        self.rect = self.image.get_rect()
        self.rect.x = x  # Spawn off-screen right
        self.rect.y = y
        
    def update(self):
        self.rect.x += self.speed_x  # Move left
        
        # Remove if off-screen left (player can't hit it)
        if self.rect.right < 0:
            self.kill()

    def take_damage(self, damage):
        self.health -= damage
        if self.health <= 0:
            player.score += self.score_value
            self.kill()

class ForcePod(pygame.sprite.Sprite):
    def __init__(self, x, y):
        super().__init__()
        self.image = pygame.Surface((20, 15))
        self.image.fill(YELLOW)
        pygame.draw.circle(self.image, YELLOW, (10, 7), 8)
        self.rect = self.image.get_rect()
        self.rect.centerx = x
        self.rect.centery = y
        self.offset_x = 15  # Attached to right side of player
        self.offset_y = -3
        
    def update_position(self, player_x, player_y):
        self.rect.centerx = player_x + self.offset_x
        self.rect.centery = player_y + self.offset_y
class PowerUp(pygame.sprite.Sprite):
    def __init__(self, x, y, power_type):
        super().__init__()
        self.power_type = power_type
        if power_type == "force_pod":
            self.image = pygame.Surface((25, 25))
            self.image.fill(YELLOW)
            pygame.draw.circle(self.image, YELLOW, (12, 12), 10)
            self.text = "Force Pod"
        elif power_type == "laser":
            self.image = pygame.Surface((25, 10))
            self.image.fill(GREEN)
            self.text = "Laser"
        elif power_type == "speed":
            self.image = pygame.Surface((25, 25))
            self.image.fill(BLUE)
            pygame.draw.polygon(self.image, BLUE, [(12, 0), (25, 12), (12, 24), (0, 12)])
            self.text = "Speed"
        
        self.rect = self.image.get_rect()
        self.rect.x = x
        self.rect.y = y
        self.speed_x = -2  # Move left with enemies
        
    def update(self):
        self.rect.x += self.speed_x  # Scroll left
        if self.rect.right < 0:
            self.kill()

def spawn_enemies(wave):
    # Spawn enemies in a horizontal wave
    for i in range(3 + wave * 2):
        x = WIDTH + random.randint(50, 200)  # Spawn off-screen right
        y = random.randint(50, HEIGHT - 100)
        enemy_type = "basic" if random.random() > 0.1 else "boss"
        enemy = Enemy(x, y, enemy_type)
        all_sprites.add(enemy)
        enemies.add(enemy)

def spawn_powerup():
    if random.random() < 0.1:  # 10% chance per frame
        x = WIDTH + random.randint(50, 200)
        y = random.randint(50, HEIGHT - 100)
        power_type = random.choice(["force_pod", "laser", "speed"])
        powerup = PowerUp(x, y, power_type)
        all_sprites.add(powerup)
        powerups.add(powerup)

# Background: scrolling stars
class Star(pygame.sprite.Sprite):
    def __init__(self):
        super().__init__()
        self.size = random.randint(1, 3)
        self.image = pygame.Surface((self.size, self.size))
        self.image.fill(WHITE)
        self.rect = self.image.get_rect()
        self.rect.x = random.randint(0, WIDTH)
        self.rect.y = random.randint(0, HEIGHT)
        self.speed_x = -1  # Move left
    
    def update(self):
        self.rect.x += self.speed_x
        if self.rect.right < 0:
            self.rect.left = WIDTH
            self.rect.y = random.randint(0, HEIGHT)

# Sprites groups
all_sprites = pygame.sprite.Group()
enemies = pygame.sprite.Group()
bullets = pygame.sprite.Group()
powerups = pygame.sprite.Group()
stars = pygame.sprite.Group()

# Create stars (background)
for _ in range(100):
    star = Star()
    stars.add(star)

# Create player
player = Player()
all_sprites.add(player)

# Game state
current_wave = 1
wave_spawned = False
spawn_timer = pygame.time.get_ticks()
spawn_interval = 3000  # ms
last_spawn_time = spawn_timer

# Scrolling background speed (fixed)
background_speed = 1
def draw_text(text, font, color, x, y):
    surf = font.render(text, True, color)
    rect = surf.get_rect()
    rect.center = (x, y)
    screen.blit(surf, rect)

def game_over_screen():
    screen.fill(BLACK)
    draw_text("GAME OVER", font_large, RED, WIDTH//2, HEIGHT//3)
    draw_text(f"Final Score: {player.score}", font_small, WHITE, WIDTH//2, HEIGHT//2)
    draw_text("Press R to Restart or Q to Quit", font_small, WHITE, WIDTH//2, HEIGHT*2//3)
    pygame.display.flip()
    
    waiting = True
    while waiting:
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                pygame.quit()
                sys.exit()
            if event.type == pygame.KEYDOWN:
                if event.key == pygame.K_r:
                    waiting = False
                    return True  # Restart
                if event.key == pygame.K_q:
                    pygame.quit()
                    sys.exit()
    return False

# Main game loop
running = True
game_over = False
while running:
    clock.tick(FPS)
    
    # Event handling
    for event in pygame.event.get():
        if event.type == pygame.QUIT:
            running = False
    
    # Check for game over
    if player.health <= 0 and not game_over:
        game_over = True
        if game_over_screen():
            # Restart the game
            player = Player()
            all_sprites.empty()
            enemies.empty()
            bullets.empty()
            powerups.empty()
            stars.empty()
            for _ in range(100):
                star = Star()
                stars.add(star)
            all_sprites.add(player)
            current_wave = 1
            game_over = False
        continue
    
    # Spawn enemies on wave start
    if not wave_spawned and len(enemies) == 0:
        spawn_enemies(current_wave)
        wave_spawned = True
        current_wave += 1
    
    # Spawn power-ups randomly
    spawn_powerup()
    
    # Update all sprites
    stars.update()
    all_sprites.update()
    
    # Check bullet-enemy collisions
    hits = pygame.sprite.groupcollide(bullets, enemies, True, False)
    for bullet, enemy_list in hits.items():
        for enemy in enemy_list:
            enemy.take_damage(1)
    
    # Check player-powerup collisions
    powerup_hits = pygame.sprite.spritecollide(player, powerups, True)
    for powerup in powerup_hits:
        player.add_powerup(powerup.power_type)
    
    # Draw everything
    screen.fill(BLACK)
    stars.draw(screen)  # Draw background first
    all_sprites.draw(screen)
    
    # UI: Health, Score, Wave
    draw_text(f"Health: {player.health}", font_small, WHITE, 100, 20)
    draw_text(f"Score: {player.score}", font_small, WHITE, WIDTH - 100, 20)
    draw_text(f"Wave: {current_wave}", font_small, WHITE, WIDTH//2, 20)
    
    # Draw power-up indicator
    if player.power_level == 1:
        draw_text("Force Pod Active", font_small, YELLOW, WIDTH//2, HEIGHT - 30)
    elif player.power_level == 2:
        draw_text("Laser Active", font_small, GREEN, WIDTH//2, HEIGHT - 30)
    
    pygame.display.flip()

pygame.quit()
sys.exit()