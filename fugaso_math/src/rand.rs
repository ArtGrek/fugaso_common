use crate::config::{ BaseConfig, ReelDist,
};
use essential_rand::error::RandError;
use essential_rand::err_on;
use essential_rand::random::RandomGenerator;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, RandError>;

pub trait ReelRandom {
    fn rand_cols(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> (Vec<usize>, Vec<Vec<char>>);
}

pub struct BaseRandom<C: BaseConfig> {
    pub rand: RandomGenerator,
    pub rows: usize,
    pub config: Arc<C>,
}

impl<C: BaseConfig> ReelRandom for BaseRandom<C> {
    fn rand_cols(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> (Vec<usize>, Vec<Vec<char>>) {
        let reels = self.config.reels();
        let reels_on = &reels[category];
        let stops = if let Some(s) = combos.filter(|s| s.len() == reels_on.len()) {
            s
        } else {
            reels_on
                .iter()
                .map(|r| self.rand.random(0, r.len()))
                .collect::<Vec<_>>()
        };

        let grid = reels_on
            .iter()
            .enumerate()
            .map(|(p, reel)| {
                let s = stops[p];
                (0..self.rows)
                    .map(|r| reel[(s + r) % reel.len()])
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        (stops, grid)
    }
}

pub struct GroupRandom<C: BaseConfig> {
    pub reels_cfg: Arc<ReelDist>,
    pub base: BaseRandom<C>,
}

impl<C: BaseConfig> GroupRandom<C> {
    pub fn rand_cols_group(
        &mut self,
        category: usize,
        combos: Option<Vec<usize>>,
    ) -> Result<(Vec<usize>, Vec<Vec<char>>)> {
        
        let reels = &self.reels_cfg[category];

        let result = if let Some(s) = combos.filter(|s| s.len() == reels.len()) {
            let grid = reels.iter().enumerate().filter_map(|(c, r)| {
                    r.iter().enumerate().find_map(|(i, e)| {
                        if i == s[c] {Some(e.1.clone())} else {None}
                    })
                }).collect::<Vec<_>>();
            if grid.len() != s.len() {return Err(err_on!("wrong combo len!"));}
            (s, grid)
        } else {
            let stops_grid = reels.iter().map(|dist| {
                self.base.rand.rand_value_clone(dist)
            }).collect::<Result<Vec<_>>>()?;
            (
                stops_grid.iter().map(|p| p.0).collect(),
                stops_grid.into_iter().map(|p| p.1).collect(),
            )
        };

        Ok(result)
    }

    pub fn rand_cols(&mut self, category: usize, combos: Option<Vec<usize>>, ) -> (Vec<usize>, Vec<Vec<char>>) {
        let reels = self.base.config.reels();
        let reel_link = &reels[category];
        let reels0 = &reels[0];
        let stops = if let Some(s) = combos.filter(|s| s.len() == reel_link.len()) {
            s
        } else {
            reel_link.iter().map(|reel| self.base.rand.random(0, reel.len())).collect::<Vec<_>>()
        };
        let grid = (0..reels0.len()).map(|c| {
            (0..self.base.rows).map(|r| {
                let index = c * self.base.rows + r;
                let s = stops[index];
                reel_link[index][s]
            }).collect::<Vec<_>>()
        }).collect::<Vec<_>>();
        (stops, grid)
    }

    pub fn config(&self)-> &C {
        &self.base.config
    }
}