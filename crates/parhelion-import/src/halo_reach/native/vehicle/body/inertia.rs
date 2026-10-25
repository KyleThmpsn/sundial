//! Cooked source tensors, rotated and combined in a common object frame.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub(super) type Tensor = [[f64; 3]; 3];

pub(super) fn read(value: &Value, metres: f32) -> Result<Tensor> {
    let values = value.as_array().context("Source body inertia")?;
    ensure!(values.len() == 12, "Source body inertia width differs");
    let mut tensor = [[0.; 3]; 3];
    for row in 0..3 {
        ensure!(
            values[row * 4 + 3].as_f64() == Some(0.),
            "Source inertia padding differs"
        );
        for axis in 0..3 {
            tensor[row][axis] = values[row * 4 + axis]
                .as_f64()
                .context("Source inertia number")?
                * f64::from(metres).powi(2);
        }
    }
    validate(&tensor)?;
    Ok(tensor)
}

pub(super) fn rotate(tensor: &Tensor, transform: &[f32; 16]) -> Tensor {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..3)
                .flat_map(|i| {
                    (0..3).map(move |j| {
                        f64::from(transform[i * 4 + row])
                            * tensor[i][j]
                            * f64::from(transform[j * 4 + column])
                    })
                })
                .sum()
        })
    })
}

pub(super) fn add(
    total: &mut Tensor,
    tensor: &Tensor,
    mass: f32,
    position: [f32; 3],
    center: [f32; 3],
) {
    let distance: [f64; 3] = std::array::from_fn(|i| f64::from(position[i]) - f64::from(center[i]));
    let squared = distance.iter().map(|v| v * v).sum::<f64>();
    for row in 0..3 {
        for column in 0..3 {
            total[row][column] += tensor[row][column]
                + f64::from(mass)
                    * (if row == column { squared } else { 0. } - distance[row] * distance[column]);
        }
    }
}

pub(super) fn validate(tensor: &Tensor) -> Result<()> {
    ensure!(
        tensor.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite vehicle inertia"
    );
    let scale = (0..3).map(|i| tensor[i][i]).fold(0f64, f64::max);
    ensure!(
        scale > 0.
            && (0..3)
                .all(|r| (0..3).all(|c| (tensor[r][c] - tensor[c][r]).abs() <= scale * 0.00001)),
        "Vehicle inertia is not symmetric"
    );
    let minor = tensor[0][0] * tensor[1][1] - tensor[0][1] * tensor[1][0];
    let determinant = tensor[0][0] * (tensor[1][1] * tensor[2][2] - tensor[1][2] * tensor[2][1])
        - tensor[0][1] * (tensor[1][0] * tensor[2][2] - tensor[1][2] * tensor[2][0])
        + tensor[0][2] * (tensor[1][0] * tensor[2][1] - tensor[1][1] * tensor[2][0]);
    ensure!(
        tensor[0][0] > 0. && minor > 0. && determinant > 0.,
        "Vehicle inertia is not positive definite"
    );
    Ok(())
}
